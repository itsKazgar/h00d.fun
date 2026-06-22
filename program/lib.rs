// h00d_market — bonding-curve launches + floor-locked P2P, for h00d.fun
//
// Two systems in one program, sharing the same SPL plumbing:
//
//   1. Bonding curve (Pump.fun-style constant product with virtual reserves).
//      A token launches into a curve; price rises as people buy, falls as they
//      sell. When the curve fills (real SOL hits the graduation threshold) it
//      "completes" and trading on the curve stops, ready for a Raydium pool.
//
//   2. Floor-locked P2P (the Ordinals-style ratchet). Sellers escrow tokens and
//      list at a price-per-unit that may never be below the all-time floor. Each
//      sale at a new high ratchets the floor up — it can only ever rise.
//
// SAFETY: this program holds user funds (SOL reserves + escrowed tokens). It is
// a v1 implementation. Test on devnet and get a professional audit before
// putting real money through it on mainnet. See the repo README.
//
// KNOWN v1 TRADEOFFS (review before mainnet):
//   * Floor ratchet: single-wallet wash-trading is blocked (buy_listing rejects
//     buyer == seller), but a determined actor with two funded wallets could still
//     ratchet the floor. Consider also capping the per-sale ratchet step.
//   * After a curve graduates, leftover tokens in the token_vault are not
//     recoverable by any instruction (only SOL leaves, via withdraw_graduated).
//   * Graduation can overshoot: a single large buy may push real_sol past
//     GRADUATION_LAMPORTS. That raises more than the target but is otherwise safe.

use anchor_lang::prelude::*;
use anchor_lang::system_program;
use anchor_spl::associated_token::AssociatedToken;
use anchor_spl::token::{self, Mint, Token, TokenAccount, Transfer};

// Placeholder program id — replace with your own keypair's pubkey before deploy
// (see the repo README). Must be valid base58 (no 0/O/I/l), so it can't spell
// "h00d" literally.
declare_id!("Fg6PaFpoGXkYsidMpWTK6W2BeZ7FEfcYkg476zPFsLnS");

// ── platform config ─────────────────────────────────────────────────────────
// Replace FEE_RECIPIENT with YOUR Solana address before building. All platform
// fees (curve trades + P2P trades) are paid to this wallet. (Placeholder below
// is the System Program id — obviously not a real recipient.)
pub const FEE_RECIPIENT: Pubkey = pubkey!("3S4VNaMUDBjbD9AkwGztmis9QUfyXfweYyzim4Vy3Y5f");

pub const FEE_BPS: u64 = 100;                       // 1.0% platform fee
pub const BPS_DENOM: u64 = 10_000;
pub const INITIAL_VIRTUAL_SOL: u64 = 30_000_000_000; // 30 SOL of virtual liquidity
pub const GRADUATION_LAMPORTS: u64 = 85_000_000_000; // graduate at 85 real SOL

#[program]
pub mod h00d_market {
    use super::*;

    // ── bonding curve ────────────────────────────────────────────────────────

    /// Launch a token into a bonding curve. The creator deposits the full
    /// `supply` of tokens into the curve's vault; the curve sells them out.
    pub fn create_curve(ctx: Context<CreateCurve>, supply: u64) -> Result<()> {
        require!(supply > 0, MarketError::BadAmount);

        // pull the whole supply from the creator into the curve vault
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.creator_token.to_account_info(),
                    to: ctx.accounts.token_vault.to_account_info(),
                    authority: ctx.accounts.creator.to_account_info(),
                },
            ),
            supply,
        )?;

        // Fund the SOL vault with the rent-exempt minimum so this system-owned
        // PDA exists immediately and can never be garbage-collected when its
        // balance is low. This rent stays in the vault and is NOT counted as
        // real_sol, so it never distorts the curve or gets paid out to traders.
        let rent_min = Rent::get()?.minimum_balance(0);
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.creator.to_account_info(),
                    to: ctx.accounts.sol_vault.to_account_info(),
                },
            ),
            rent_min,
        )?;

        let c = &mut ctx.accounts.curve;
        c.creator = ctx.accounts.creator.key();
        c.mint = ctx.accounts.mint.key();
        c.virtual_sol = INITIAL_VIRTUAL_SOL;
        c.virtual_token = supply;
        c.real_sol = 0;
        c.real_token = supply;
        c.complete = false;
        c.bump = ctx.bumps.curve;
        c.vault_bump = ctx.bumps.sol_vault;
        Ok(())
    }

    /// Buy tokens from the curve with `sol_in` lamports. Price rises as the
    /// curve's SOL reserve grows (constant product: x*y = k).
    pub fn buy(ctx: Context<Trade>, sol_in: u64, min_tokens_out: u64) -> Result<()> {
        // snapshot the curve so we don't hold a &mut across the CPIs
        let (v_sol, v_token, real_token, complete, bump, mint_key) = {
            let c = &ctx.accounts.curve;
            (c.virtual_sol, c.virtual_token, c.real_token, c.complete, c.bump, c.mint)
        };
        require!(!complete, MarketError::CurveComplete);
        require!(sol_in > 0, MarketError::BadAmount);

        let fee = sol_in.checked_mul(FEE_BPS).ok_or(MarketError::Math)? / BPS_DENOM;
        let sol_net = sol_in.checked_sub(fee).ok_or(MarketError::Math)?;

        // tokens_out = virtual_token - k / (virtual_sol + sol_net)
        let k = (v_sol as u128).checked_mul(v_token as u128).ok_or(MarketError::Math)?;
        let new_v_sol = (v_sol as u128).checked_add(sol_net as u128).ok_or(MarketError::Math)?;
        let new_v_token = k.checked_div(new_v_sol).ok_or(MarketError::Math)?;
        let tokens_out = (v_token as u128).checked_sub(new_v_token).ok_or(MarketError::Math)? as u64;

        require!(tokens_out > 0, MarketError::Math);
        require!(tokens_out <= real_token, MarketError::CurveDrained);
        require!(tokens_out >= min_tokens_out, MarketError::Slippage);

        // buyer pays: net to the SOL vault, fee to the platform
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.user.to_account_info(),
                    to: ctx.accounts.sol_vault.to_account_info(),
                },
            ),
            sol_net,
        )?;
        if fee > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.user.to_account_info(),
                        to: ctx.accounts.fee_recipient.to_account_info(),
                    },
                ),
                fee,
            )?;
        }

        // curve sends tokens to the buyer (vault is owned by the curve PDA)
        let seeds: &[&[u8]] = &[b"curve", mint_key.as_ref(), &[bump]];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.token_vault.to_account_info(),
                    to: ctx.accounts.user_token.to_account_info(),
                    authority: ctx.accounts.curve.to_account_info(),
                },
                &[seeds],
            ),
            tokens_out,
        )?;

        // commit new reserves
        let user_key = ctx.accounts.user.key();
        let c = &mut ctx.accounts.curve;
        c.virtual_sol = c.virtual_sol.checked_add(sol_net).ok_or(MarketError::Math)?;
        c.virtual_token = c.virtual_token.checked_sub(tokens_out).ok_or(MarketError::Math)?;
        c.real_sol = c.real_sol.checked_add(sol_net).ok_or(MarketError::Math)?;
        c.real_token = c.real_token.checked_sub(tokens_out).ok_or(MarketError::Math)?;
        let graduated = c.real_sol >= GRADUATION_LAMPORTS;
        if graduated {
            c.complete = true;
        }
        let (real_sol_now, real_token_now) = (c.real_sol, c.real_token);
        emit!(CurveTraded {
            mint: mint_key, user: user_key, is_buy: true,
            sol_amount: sol_in, token_amount: tokens_out,
            real_sol: real_sol_now, real_token: real_token_now,
        });
        if graduated {
            emit!(CurveGraduated { mint: mint_key, real_sol: real_sol_now });
        }
        Ok(())
    }

    /// Sell `tokens_in` tokens back to the curve for SOL. Price falls.
    pub fn sell(ctx: Context<Trade>, tokens_in: u64, min_sol_out: u64) -> Result<()> {
        let (v_sol, v_token, real_sol, complete, vault_bump, mint_key) = {
            let c = &ctx.accounts.curve;
            (c.virtual_sol, c.virtual_token, c.real_sol, c.complete, c.vault_bump, c.mint)
        };
        require!(!complete, MarketError::CurveComplete);
        require!(tokens_in > 0, MarketError::BadAmount);

        // sol_out = virtual_sol - k / (virtual_token + tokens_in)
        let k = (v_sol as u128).checked_mul(v_token as u128).ok_or(MarketError::Math)?;
        let new_v_token = (v_token as u128).checked_add(tokens_in as u128).ok_or(MarketError::Math)?;
        let new_v_sol = k.checked_div(new_v_token).ok_or(MarketError::Math)?;
        let gross = (v_sol as u128).checked_sub(new_v_sol).ok_or(MarketError::Math)? as u64;
        require!(gross > 0 && gross <= real_sol, MarketError::CurveDrained);

        let fee = gross.checked_mul(FEE_BPS).ok_or(MarketError::Math)? / BPS_DENOM;
        let sol_out = gross.checked_sub(fee).ok_or(MarketError::Math)?;
        require!(sol_out >= min_sol_out, MarketError::Slippage);

        // seller sends tokens into the vault
        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.user_token.to_account_info(),
                    to: ctx.accounts.token_vault.to_account_info(),
                    authority: ctx.accounts.user.to_account_info(),
                },
            ),
            tokens_in,
        )?;

        // vault pays the seller (net) and the platform (fee)
        let vseeds: &[&[u8]] = &[b"sol_vault", mint_key.as_ref(), &[vault_bump]];
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.sol_vault.to_account_info(),
                    to: ctx.accounts.user.to_account_info(),
                },
                &[vseeds],
            ),
            sol_out,
        )?;
        if fee > 0 {
            system_program::transfer(
                CpiContext::new_with_signer(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.sol_vault.to_account_info(),
                        to: ctx.accounts.fee_recipient.to_account_info(),
                    },
                    &[vseeds],
                ),
                fee,
            )?;
        }

        let user_key = ctx.accounts.user.key();
        let c = &mut ctx.accounts.curve;
        c.virtual_sol = c.virtual_sol.checked_sub(gross).ok_or(MarketError::Math)?;
        c.virtual_token = c.virtual_token.checked_add(tokens_in).ok_or(MarketError::Math)?;
        c.real_sol = c.real_sol.checked_sub(gross).ok_or(MarketError::Math)?;
        c.real_token = c.real_token.checked_add(tokens_in).ok_or(MarketError::Math)?;
        let (real_sol_now, real_token_now) = (c.real_sol, c.real_token);
        emit!(CurveTraded {
            mint: mint_key, user: user_key, is_buy: false,
            sol_amount: sol_out, token_amount: tokens_in,
            real_sol: real_sol_now, real_token: real_token_now,
        });
        Ok(())
    }

    /// Withdraw the SOL collected by a curve once it has GRADUATED, so it can be
    /// used to seed a DEX (Raydium) pool. Without this, the SOL a successful
    /// curve raised would be stuck in the vault forever. Only the platform
    /// authority (FEE_RECIPIENT) may call it, and only after graduation. The
    /// rent-exempt minimum stays in the vault; only `real_sol` can leave.
    pub fn withdraw_graduated(ctx: Context<WithdrawGraduated>, amount: u64) -> Result<()> {
        let (real_sol, complete, vault_bump, mint_key) = {
            let c = &ctx.accounts.curve;
            (c.real_sol, c.complete, c.vault_bump, c.mint)
        };
        require!(complete, MarketError::NotGraduated);
        require!(amount > 0 && amount <= real_sol, MarketError::BadAmount);

        let vseeds: &[&[u8]] = &[b"sol_vault", mint_key.as_ref(), &[vault_bump]];
        system_program::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.sol_vault.to_account_info(),
                    to: ctx.accounts.authority.to_account_info(),
                },
                &[vseeds],
            ),
            amount,
        )?;

        let c = &mut ctx.accounts.curve;
        c.real_sol = c.real_sol.checked_sub(amount).ok_or(MarketError::Math)?;
        Ok(())
    }

    // ── floor-locked P2P ──────────────────────────────────────────────────────

    /// One-time init of a mint's floor tracker (starts at 0 = no floor yet).
    pub fn init_floor(ctx: Context<InitFloor>) -> Result<()> {
        let f = &mut ctx.accounts.floor;
        f.mint = ctx.accounts.mint.key();
        f.floor = 0;
        f.bump = ctx.bumps.floor;
        Ok(())
    }

    /// List `amount` base-units of a token at `price_per_unit` lamports each.
    /// The price may never be below the current floor. Tokens are escrowed.
    pub fn create_listing(
        ctx: Context<CreateListing>,
        seed: u64,
        amount: u64,
        price_per_unit: u64,
    ) -> Result<()> {
        require!(amount > 0 && price_per_unit > 0, MarketError::BadAmount);
        require!(price_per_unit >= ctx.accounts.floor.floor, MarketError::BelowFloor);
        // guard against overflow on the eventual total
        let _total = (price_per_unit as u128).checked_mul(amount as u128).ok_or(MarketError::Math)?;

        token::transfer(
            CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.seller_token.to_account_info(),
                    to: ctx.accounts.escrow.to_account_info(),
                    authority: ctx.accounts.seller.to_account_info(),
                },
            ),
            amount,
        )?;

        let l = &mut ctx.accounts.listing;
        l.seller = ctx.accounts.seller.key();
        l.mint = ctx.accounts.mint.key();
        l.amount = amount;
        l.price_per_unit = price_per_unit;
        l.seed = seed;
        l.bump = ctx.bumps.listing;
        emit!(ListingCreated {
            mint: l.mint, seller: l.seller,
            price_per_unit, amount, seed,
        });
        Ok(())
    }

    /// Buy out a listing entirely. Buyer pays seller (net) + platform (fee),
    /// receives the escrowed tokens, and the floor ratchets up to this price.
    pub fn buy_listing(ctx: Context<BuyListing>) -> Result<()> {
        // snapshot listing fields into locals (no borrow held across CPIs)
        let amount = ctx.accounts.listing.amount;
        let price = ctx.accounts.listing.price_per_unit;
        let mint_key = ctx.accounts.listing.mint;
        let seller_key = ctx.accounts.listing.seller;
        let seed_bytes = ctx.accounts.listing.seed.to_le_bytes();
        let l_bump = ctx.accounts.listing.bump;
        // reject self-trades: buyer and seller must differ. This blocks a single
        // wallet from wash-ratcheting the floor against its own listing.
        let buyer_key = ctx.accounts.buyer.key();
        require!(buyer_key != seller_key, MarketError::SelfTrade);

        let total = (price as u128).checked_mul(amount as u128).ok_or(MarketError::Math)? as u64;
        let fee = total.checked_mul(FEE_BPS).ok_or(MarketError::Math)? / BPS_DENOM;
        let to_seller = total.checked_sub(fee).ok_or(MarketError::Math)?;

        // buyer pays seller and platform
        system_program::transfer(
            CpiContext::new(
                ctx.accounts.system_program.to_account_info(),
                system_program::Transfer {
                    from: ctx.accounts.buyer.to_account_info(),
                    to: ctx.accounts.seller.to_account_info(),
                },
            ),
            to_seller,
        )?;
        if fee > 0 {
            system_program::transfer(
                CpiContext::new(
                    ctx.accounts.system_program.to_account_info(),
                    system_program::Transfer {
                        from: ctx.accounts.buyer.to_account_info(),
                        to: ctx.accounts.fee_recipient.to_account_info(),
                    },
                ),
                fee,
            )?;
        }

        // escrow releases tokens to the buyer (escrow owned by the listing PDA)
        let seeds: &[&[u8]] = &[
            b"listing", mint_key.as_ref(), seller_key.as_ref(), &seed_bytes, &[l_bump],
        ];
        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow.to_account_info(),
                    to: ctx.accounts.buyer_token.to_account_info(),
                    authority: ctx.accounts.listing.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;

        // enforce the invariant at execution time: nothing trades below the
        // current floor. A listing made when the floor was lower is no longer
        // buyable once the floor rises — the seller must cancel and relist.
        let f = &mut ctx.accounts.floor;
        require!(price >= f.floor, MarketError::BelowFloor);
        // ratchet the floor — it can only ever rise (self-trades already rejected).
        if price > f.floor {
            f.floor = price;
        }
        let new_floor = f.floor;
        emit!(ListingFilled {
            mint: mint_key, seller: seller_key, buyer: buyer_key,
            price_per_unit: price, amount, new_floor,
        });

        // close escrow token account, refunding rent to the seller
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::CloseAccount {
                account: ctx.accounts.escrow.to_account_info(),
                destination: ctx.accounts.seller.to_account_info(),
                authority: ctx.accounts.listing.to_account_info(),
            },
            &[seeds],
        ))?;
        Ok(())
    }

    /// Seller cancels their listing and gets the escrowed tokens back.
    pub fn cancel_listing(ctx: Context<CancelListing>) -> Result<()> {
        let amount = ctx.accounts.listing.amount;
        let mint_key = ctx.accounts.listing.mint;
        let seller_key = ctx.accounts.listing.seller;
        let seed_bytes = ctx.accounts.listing.seed.to_le_bytes();
        let l_bump = ctx.accounts.listing.bump;
        let seeds: &[&[u8]] = &[
            b"listing", mint_key.as_ref(), seller_key.as_ref(), &seed_bytes, &[l_bump],
        ];

        token::transfer(
            CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                Transfer {
                    from: ctx.accounts.escrow.to_account_info(),
                    to: ctx.accounts.seller_token.to_account_info(),
                    authority: ctx.accounts.listing.to_account_info(),
                },
                &[seeds],
            ),
            amount,
        )?;
        token::close_account(CpiContext::new_with_signer(
            ctx.accounts.token_program.to_account_info(),
            token::CloseAccount {
                account: ctx.accounts.escrow.to_account_info(),
                destination: ctx.accounts.seller.to_account_info(),
                authority: ctx.accounts.listing.to_account_info(),
            },
            &[seeds],
        ))?;
        Ok(())
    }
}

// ── accounts ─────────────────────────────────────────────────────────────────

#[derive(Accounts)]
pub struct CreateCurve<'info> {
    #[account(mut)]
    pub creator: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        init, payer = creator, space = 8 + Curve::LEN,
        seeds = [b"curve", mint.key().as_ref()], bump
    )]
    pub curve: Account<'info, Curve>,
    /// CHECK: system-owned PDA that only holds the curve's SOL reserve
    #[account(
        mut, seeds = [b"sol_vault", mint.key().as_ref()], bump
    )]
    pub sol_vault: UncheckedAccount<'info>,
    #[account(
        init, payer = creator,
        associated_token::mint = mint, associated_token::authority = curve
    )]
    pub token_vault: Account<'info, TokenAccount>,
    #[account(mut, constraint = creator_token.mint == mint.key())]
    pub creator_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Trade<'info> {
    #[account(mut)]
    pub user: Signer<'info>,
    #[account(
        mut, seeds = [b"curve", curve.mint.as_ref()], bump = curve.bump
    )]
    pub curve: Account<'info, Curve>,
    /// CHECK: system-owned PDA holding the curve's SOL reserve
    #[account(
        mut, seeds = [b"sol_vault", curve.mint.as_ref()], bump = curve.vault_bump
    )]
    pub sol_vault: UncheckedAccount<'info>,
    #[account(
        mut, associated_token::mint = mint, associated_token::authority = curve
    )]
    pub token_vault: Account<'info, TokenAccount>,
    #[account(mut, constraint = user_token.mint == curve.mint)]
    pub user_token: Account<'info, TokenAccount>,
    pub mint: Account<'info, Mint>,
    /// CHECK: validated to equal the platform FEE_RECIPIENT
    #[account(mut, address = FEE_RECIPIENT)]
    pub fee_recipient: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct WithdrawGraduated<'info> {
    /// Only the platform authority may withdraw graduated funds.
    #[account(mut, address = FEE_RECIPIENT)]
    pub authority: Signer<'info>,
    #[account(
        mut, seeds = [b"curve", curve.mint.as_ref()], bump = curve.bump
    )]
    pub curve: Account<'info, Curve>,
    /// CHECK: system-owned PDA holding the curve's SOL reserve
    #[account(
        mut, seeds = [b"sol_vault", curve.mint.as_ref()], bump = curve.vault_bump
    )]
    pub sol_vault: UncheckedAccount<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct InitFloor<'info> {
    #[account(mut)]
    pub payer: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        init, payer = payer, space = 8 + Floor::LEN,
        seeds = [b"floor", mint.key().as_ref()], bump
    )]
    pub floor: Account<'info, Floor>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
#[instruction(seed: u64)]
pub struct CreateListing<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,
    pub mint: Account<'info, Mint>,
    #[account(
        seeds = [b"floor", mint.key().as_ref()], bump = floor.bump,
        constraint = floor.mint == mint.key()
    )]
    pub floor: Account<'info, Floor>,
    #[account(
        init, payer = seller, space = 8 + Listing::LEN,
        seeds = [b"listing", mint.key().as_ref(), seller.key().as_ref(), &seed.to_le_bytes()], bump
    )]
    pub listing: Account<'info, Listing>,
    #[account(
        init, payer = seller,
        associated_token::mint = mint, associated_token::authority = listing
    )]
    pub escrow: Account<'info, TokenAccount>,
    #[account(mut, constraint = seller_token.mint == mint.key())]
    pub seller_token: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
    pub associated_token_program: Program<'info, AssociatedToken>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct BuyListing<'info> {
    #[account(mut)]
    pub buyer: Signer<'info>,
    /// CHECK: paid out to; must match the listing's recorded seller
    #[account(mut, address = listing.seller)]
    pub seller: UncheckedAccount<'info>,
    #[account(
        mut, close = seller,
        seeds = [b"listing", listing.mint.as_ref(), listing.seller.as_ref(), &listing.seed.to_le_bytes()],
        bump = listing.bump
    )]
    pub listing: Account<'info, Listing>,
    #[account(
        mut, seeds = [b"floor", listing.mint.as_ref()], bump = floor.bump
    )]
    pub floor: Account<'info, Floor>,
    #[account(mut, associated_token::mint = mint, associated_token::authority = listing)]
    pub escrow: Account<'info, TokenAccount>,
    #[account(mut, constraint = buyer_token.mint == listing.mint)]
    pub buyer_token: Account<'info, TokenAccount>,
    pub mint: Account<'info, Mint>,
    /// CHECK: validated to equal the platform FEE_RECIPIENT
    #[account(mut, address = FEE_RECIPIENT)]
    pub fee_recipient: UncheckedAccount<'info>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct CancelListing<'info> {
    #[account(mut)]
    pub seller: Signer<'info>,
    #[account(
        mut, close = seller, has_one = seller,
        seeds = [b"listing", listing.mint.as_ref(), listing.seller.as_ref(), &listing.seed.to_le_bytes()],
        bump = listing.bump
    )]
    pub listing: Account<'info, Listing>,
    #[account(mut, associated_token::mint = mint, associated_token::authority = listing)]
    pub escrow: Account<'info, TokenAccount>,
    #[account(mut, constraint = seller_token.mint == listing.mint)]
    pub seller_token: Account<'info, TokenAccount>,
    pub mint: Account<'info, Mint>,
    pub token_program: Program<'info, Token>,
    pub system_program: Program<'info, System>,
}

// ── state ────────────────────────────────────────────────────────────────────

#[account]
pub struct Curve {
    pub creator: Pubkey,
    pub mint: Pubkey,
    pub virtual_sol: u64,
    pub virtual_token: u64,
    pub real_sol: u64,
    pub real_token: u64,
    pub complete: bool,
    pub bump: u8,
    pub vault_bump: u8,
}
impl Curve { pub const LEN: usize = 32 + 32 + 8 + 8 + 8 + 8 + 1 + 1 + 1; }

#[account]
pub struct Floor {
    pub mint: Pubkey,
    pub floor: u64,
    pub bump: u8,
}
impl Floor { pub const LEN: usize = 32 + 8 + 1; }

#[account]
pub struct Listing {
    pub seller: Pubkey,
    pub mint: Pubkey,
    pub amount: u64,
    pub price_per_unit: u64,
    pub seed: u64,
    pub bump: u8,
}
impl Listing { pub const LEN: usize = 32 + 32 + 8 + 8 + 8 + 1; }

// ── errors ───────────────────────────────────────────────────────────────────

#[error_code]
pub enum MarketError {
    #[msg("amount must be greater than zero")]
    BadAmount,
    #[msg("arithmetic overflow")]
    Math,
    #[msg("curve has graduated; trade on the DEX")]
    CurveComplete,
    #[msg("not enough liquidity in the curve")]
    CurveDrained,
    #[msg("output below your minimum (slippage)")]
    Slippage,
    #[msg("price is below the all-time floor")]
    BelowFloor,
    #[msg("curve has not graduated yet")]
    NotGraduated,
    #[msg("buyer and seller must differ")]
    SelfTrade,
}

// ── events ───────────────────────────────────────────────────────────────────
// Emitted via Anchor's `emit!` so indexers and the UI can follow activity from
// transaction logs instead of scraping account state.

#[event]
pub struct CurveTraded {
    pub mint: Pubkey,
    pub user: Pubkey,
    pub is_buy: bool,
    pub sol_amount: u64,
    pub token_amount: u64,
    pub real_sol: u64,
    pub real_token: u64,
}

#[event]
pub struct CurveGraduated {
    pub mint: Pubkey,
    pub real_sol: u64,
}

#[event]
pub struct ListingCreated {
    pub mint: Pubkey,
    pub seller: Pubkey,
    pub price_per_unit: u64,
    pub amount: u64,
    pub seed: u64,
}

#[event]
pub struct ListingFilled {
    pub mint: Pubkey,
    pub seller: Pubkey,
    pub buyer: Pubkey,
    pub price_per_unit: u64,
    pub amount: u64,
    pub new_floor: u64,
}
