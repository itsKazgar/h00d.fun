# Deploying the h00d_market program

The site's **swap / mint / discover / feed** already work without this. Only
**market** and **curve** need the on-chain program. Two ways to deploy it —
Playground (easiest, no install) or the Anchor CLI (for testing first).

> ⚠️ It holds user funds. **Deploy to devnet, test every action, and ideally get
> an audit before mainnet.** Devnet SOL is free (airdrop). Mainnet deploy costs a
> few real SOL (the program's on-chain "rent").

---

## A) Easiest — Solana Playground (browser, no install)

1. Go to **https://beta.solpg.io** → create a new **Anchor** project.
2. Replace `src/lib.rs` with the contents of this repo's **`program/lib.rs`**.
3. **Set your two constants** in that file:
   - `pub const FEE_RECIPIENT: Pubkey = pubkey!("YOUR_FEE_WALLET");`
   - leave `declare_id!(...)` for now — Playground fills it after the first build.
4. Top-left: connect a wallet (Playground can make a throwaway one) and switch the
   network to **devnet** (bottom bar). Click **Airdrop** a couple times for test SOL.
5. **Build** (🔨). Playground prints your **program id** — paste it into
   `declare_id!("...")`, then build once more.
6. **Deploy** (⚡). Wait for "Deployment successful".
7. Copy the **program id** and paste it into **`PROGRAM_ID_STR`** in both
   `market.html` and `curve.html` (and confirm `FEE_RECIPIENT_STR` matches your
   fee wallet). Commit + push — market/curve go live.
8. When you're happy on devnet, switch the network to **mainnet-beta**, fund the
   wallet with real SOL, and **Deploy** again. Update the program id if it changed.

---

## B) Anchor CLI (lets you run the tests first)

Prereqs: Rust, Solana CLI, Anchor (`avm install 0.30.1 && avm use 0.30.1`), Node.

```bash
cd anchor
npm install                      # test deps

# run the full test suite on a local validator
anchor test

# devnet deploy
solana config set --url devnet
solana airdrop 2
anchor build
anchor keys sync                 # writes the real program id into Anchor.toml + declare_id!
anchor build
anchor deploy --provider.cluster devnet
```

`anchor build` reads the program from `programs/h00d_market/Cargo.toml`, which
points at the single source of truth, **`program/lib.rs`** (so there's no second
copy to keep in sync). After deploy, paste the program id into `PROGRAM_ID_STR`
in `market.html` and `curve.html`.

For **mainnet**: `solana config set --url mainnet-beta`, fund the wallet with real
SOL, then `anchor deploy --provider.cluster mainnet`.

---

## After deploying (both paths)

- [ ] `PROGRAM_ID_STR` set in `market.html` **and** `curve.html`
- [ ] `FEE_RECIPIENT` (in `lib.rs`) == `FEE_RECIPIENT_STR` (in the html)
- [ ] tested buy / sell / list / buy-listing / cancel on devnet
- [ ] (mainnet) audited, upgrade authority on a multisig — see `GO-LIVE.md`
