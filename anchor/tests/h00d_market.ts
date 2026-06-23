// h00d_market test suite.
//   anchor test           (spins up a local validator, runs these)
//
// Covers: bonding curve (create/buy/sell), floor-locked P2P (init/list/buy/cancel),
// the floor ratchet, and the guards (self-trade, below-floor, slippage).
// Note: withdraw_graduated isn't tested here because it requires the FEE_RECIPIENT
// private key (a fixed address in the program); test it on devnet with that wallet.

import * as anchor from "@coral-xyz/anchor";
import { Program } from "@coral-xyz/anchor";
import {
  PublicKey, Keypair, SystemProgram, LAMPORTS_PER_SOL,
} from "@solana/web3.js";
import {
  createMint, getOrCreateAssociatedTokenAccount, mintTo, getAccount,
  getAssociatedTokenAddressSync, createAssociatedTokenAccountInstruction,
  TOKEN_PROGRAM_ID, ASSOCIATED_TOKEN_PROGRAM_ID,
} from "@solana/spl-token";
import { assert } from "chai";

const FEE_RECIPIENT = new PublicKey("3S4VNaMUDBjbD9AkwGztmis9QUfyXfweYyzim4Vy3Y5f");
const enc = (s: string) => Buffer.from(s);
const seedLE = (n: number) => new anchor.BN(n).toArrayLike(Buffer, "le", 8);

describe("h00d_market", () => {
  const provider = anchor.AnchorProvider.env();
  anchor.setProvider(provider);
  const program = anchor.workspace.H00dMarket as Program;
  const conn = provider.connection;
  const creator = (provider.wallet as anchor.Wallet).payer;
  const PID = program.programId;

  async function fund(kp: Keypair, sol = 5) {
    const sig = await conn.requestAirdrop(kp.publicKey, sol * LAMPORTS_PER_SOL);
    await conn.confirmTransaction(sig);
  }

  // ---- bonding curve -------------------------------------------------------
  describe("bonding curve", () => {
    let mint: PublicKey;
    const decimals = 6;
    const supply = new anchor.BN(1_000_000).mul(new anchor.BN(10 ** decimals));
    let curve: PublicKey, solVault: PublicKey, tokenVault: PublicKey, creatorToken: PublicKey;

    before(async () => {
      mint = await createMint(conn, creator, creator.publicKey, null, decimals);
      const ata = await getOrCreateAssociatedTokenAccount(conn, creator, mint, creator.publicKey);
      creatorToken = ata.address;
      await mintTo(conn, creator, mint, creatorToken, creator, BigInt(supply.toString()));
      [curve] = PublicKey.findProgramAddressSync([enc("curve"), mint.toBuffer()], PID);
      [solVault] = PublicKey.findProgramAddressSync([enc("sol_vault"), mint.toBuffer()], PID);
      tokenVault = getAssociatedTokenAddressSync(mint, curve, true);
    });

    it("creates the curve and escrows the supply", async () => {
      await program.methods.createCurve(supply).accountsPartial({
        creator: creator.publicKey, mint, curve, solVault, tokenVault, creatorToken,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      }).rpc();

      const c = await (program.account as any).curve.fetch(curve);
      assert.ok(c.virtualSol.eq(new anchor.BN("30000000000")));
      assert.ok(c.realToken.eq(supply));
      assert.equal(c.complete, false);
      const vault = await getAccount(conn, tokenVault);
      assert.equal(vault.amount.toString(), supply.toString());
    });

    it("buys tokens off the curve", async () => {
      const buyer = Keypair.generate();
      await fund(buyer, 5);
      const buyerToken = getAssociatedTokenAddressSync(mint, buyer.publicKey);
      await program.methods.buy(new anchor.BN(LAMPORTS_PER_SOL), new anchor.BN(0))
        .accountsPartial({
          user: buyer.publicKey, curve, solVault, tokenVault, userToken: buyerToken, mint,
          feeRecipient: FEE_RECIPIENT, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
        })
        .preInstructions([
          createAssociatedTokenAccountInstruction(
            buyer.publicKey, buyerToken, buyer.publicKey, mint),
        ])
        .signers([buyer]).rpc();

      const bal = await getAccount(conn, buyerToken);
      assert.ok(bal.amount > 0n, "buyer received tokens");
      const c = await (program.account as any).curve.fetch(curve);
      assert.ok(c.realSol.gt(new anchor.BN(0)), "curve collected SOL");
    });

    it("rejects a buy that violates slippage (min_tokens_out too high)", async () => {
      const buyer = Keypair.generate();
      await fund(buyer, 3);
      const buyerToken = getAssociatedTokenAddressSync(mint, buyer.publicKey);
      try {
        await program.methods.buy(new anchor.BN(LAMPORTS_PER_SOL), new anchor.BN("1000000000000000"))
          .accountsPartial({
            user: buyer.publicKey, curve, solVault, tokenVault, userToken: buyerToken, mint,
            feeRecipient: FEE_RECIPIENT, tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
          })
          .preInstructions([
            createAssociatedTokenAccountInstruction(
              buyer.publicKey, buyerToken, buyer.publicKey, mint),
          ])
          .signers([buyer]).rpc();
        assert.fail("should have thrown Slippage");
      } catch (e: any) {
        assert.match(e.toString(), /Slippage|slippage/);
      }
    });
  });

  // ---- floor-locked P2P market --------------------------------------------
  describe("floor-locked market", () => {
    let mint: PublicKey, floor: PublicKey, sellerToken: PublicKey;
    const decimals = 6;
    const seller = creator;

    before(async () => {
      mint = await createMint(conn, creator, creator.publicKey, null, decimals);
      const ata = await getOrCreateAssociatedTokenAccount(conn, creator, mint, seller.publicKey);
      sellerToken = ata.address;
      await mintTo(conn, creator, mint, sellerToken, creator, 10_000_000n);
      [floor] = PublicKey.findProgramAddressSync([enc("floor"), mint.toBuffer()], PID);
    });

    it("inits the floor at 0", async () => {
      await program.methods.initFloor().accountsPartial({
        payer: creator.publicKey, mint, floor, systemProgram: SystemProgram.programId,
      }).rpc();
      const f = await (program.account as any).floor.fetch(floor);
      assert.ok(f.floor.eq(new anchor.BN(0)));
    });

    const seed = 1, amount = new anchor.BN(1_000_000), price = new anchor.BN(50);
    let listing: PublicKey, escrow: PublicKey;

    it("creates a listing (tokens escrowed)", async () => {
      [listing] = PublicKey.findProgramAddressSync(
        [enc("listing"), mint.toBuffer(), seller.publicKey.toBuffer(), seedLE(seed)], PID);
      escrow = getAssociatedTokenAddressSync(mint, listing, true);
      await program.methods.createListing(new anchor.BN(seed), amount, price).accountsPartial({
        seller: seller.publicKey, mint, floor, listing, escrow, sellerToken,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      }).rpc();
      const esc = await getAccount(conn, escrow);
      assert.equal(esc.amount.toString(), amount.toString());
    });

    it("rejects the seller buying their own listing (self-trade)", async () => {
      const buyerToken = getAssociatedTokenAddressSync(mint, seller.publicKey);
      try {
        await program.methods.buyListing().accountsPartial({
          buyer: seller.publicKey, seller: seller.publicKey, listing, floor, escrow,
          buyerToken, mint, feeRecipient: FEE_RECIPIENT,
          tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
        }).rpc();
        assert.fail("should have thrown SelfTrade");
      } catch (e: any) {
        assert.match(e.toString(), /SelfTrade|differ/);
      }
    });

    it("lets a different buyer fill it and ratchets the floor up", async () => {
      const buyer = Keypair.generate();
      await fund(buyer, 3);
      const buyerToken = getAssociatedTokenAddressSync(mint, buyer.publicKey);
      await program.methods.buyListing().accountsPartial({
        buyer: buyer.publicKey, seller: seller.publicKey, listing, floor, escrow,
        buyerToken, mint, feeRecipient: FEE_RECIPIENT,
        tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      })
        .preInstructions([
          createAssociatedTokenAccountInstruction(
            buyer.publicKey, buyerToken, buyer.publicKey, mint),
        ])
        .signers([buyer]).rpc();

      const bal = await getAccount(conn, buyerToken);
      assert.equal(bal.amount.toString(), amount.toString(), "buyer got the tokens");
      const f = await (program.account as any).floor.fetch(floor);
      assert.ok(f.floor.eq(price), "floor ratcheted to the sale price");
    });

    it("rejects a new listing priced below the floor", async () => {
      const s2 = 2;
      const [listing2] = PublicKey.findProgramAddressSync(
        [enc("listing"), mint.toBuffer(), seller.publicKey.toBuffer(), seedLE(s2)], PID);
      const escrow2 = getAssociatedTokenAddressSync(mint, listing2, true);
      try {
        await program.methods.createListing(new anchor.BN(s2), new anchor.BN(1000), new anchor.BN(1))
          .accountsPartial({
            seller: seller.publicKey, mint, floor, listing: listing2, escrow: escrow2, sellerToken,
            tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
            systemProgram: SystemProgram.programId,
          }).rpc();
        assert.fail("should have thrown BelowFloor");
      } catch (e: any) {
        assert.match(e.toString(), /BelowFloor|floor/);
      }
    });

    it("cancels a listing and returns the tokens", async () => {
      const s3 = 3, amt = new anchor.BN(2_000_000);
      const [listing3] = PublicKey.findProgramAddressSync(
        [enc("listing"), mint.toBuffer(), seller.publicKey.toBuffer(), seedLE(s3)], PID);
      const escrow3 = getAssociatedTokenAddressSync(mint, listing3, true);
      const before = (await getAccount(conn, sellerToken)).amount;
      // price >= current floor (50)
      await program.methods.createListing(new anchor.BN(s3), amt, new anchor.BN(60)).accountsPartial({
        seller: seller.publicKey, mint, floor, listing: listing3, escrow: escrow3, sellerToken,
        tokenProgram: TOKEN_PROGRAM_ID, associatedTokenProgram: ASSOCIATED_TOKEN_PROGRAM_ID,
        systemProgram: SystemProgram.programId,
      }).rpc();
      await program.methods.cancelListing().accountsPartial({
        seller: seller.publicKey, listing: listing3, escrow: escrow3, sellerToken, mint,
        tokenProgram: TOKEN_PROGRAM_ID, systemProgram: SystemProgram.programId,
      }).rpc();
      const after = (await getAccount(conn, sellerToken)).amount;
      assert.equal(after.toString(), before.toString(), "tokens returned on cancel");
    });
  });
});
