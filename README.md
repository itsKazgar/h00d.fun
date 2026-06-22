# h00d.fun

**A Solana launchpad and floor-locked market.** Mint a token that ends in
`…hood`, swap any Solana token, and trade on a market where the **price floor
can only ever go up** — never down.

Non-custodial and on-chain: every action is signed in the user's own wallet, and
funds are held by the program's on-chain accounts, not by any server.

---

## What's inside

| Page | What it does | Status |
|------|--------------|--------|
| **swap** (`swap.html`) | Trade any Solana token via the Jupiter aggregator | ✅ live |
| **mint** (`launch.html`) | Launch your own `…hood` vanity token | ✅ live |
| **discover** (`discover.html`) | Browse coins launched here | ✅ live |
| **market** (`market.html`) | Floor-locked P2P escrow — the floor only ratchets up | 🕓 needs program deploy |
| **curve** (`curve.html`) | Bonding-curve launches (price rises as people buy) | 🕓 needs program deploy |

> The **swap / mint / discover** pages work today — they use Jupiter and standard
> Solana token tooling and need no custom program. The **market** and **curve**
> pages talk to the on-chain `h00d_market` program (below) and light up once it's
> deployed and its program id is set in those files.

---

## The idea: a floor that only goes up

Inspired by the Bitcoin-Ordinals "floor ratchet," the market enforces — *in the
program itself* — that no listing may be priced below the all-time floor, and
every sale at a new high pushes the floor higher. The floor can rise; it can
never fall. This is the project's distinguishing mechanism.

## Architecture (no backend by design)

```
Static site (this repo)  →  user's wallet signs  →  Solana
                                                     (h00d_market program + PDAs)
```

- **No server holds funds.** The "backend" is the Solana program in
  [`program/lib.rs`](program/lib.rs); the "database" is its on-chain accounts.
- **Non-custodial.** Users sign every transaction in their own wallet.
- **Transparent.** The full on-chain program source is included in this repo.

## The on-chain program

[`program/lib.rs`](program/lib.rs) is the Anchor program (`h00d_market`):
bonding curve (`create_curve` / `buy` / `sell` / `withdraw_graduated`) and
floor-locked P2P (`init_floor` / `create_listing` / `buy_listing` /
`cancel_listing`). It is deployed separately via Anchor / Solana Playground; once
deployed, paste its program id into `PROGRAM_ID_STR` in `market.html` and
`curve.html` to go live.

## Going live (market + curve)

1. Deploy `program/lib.rs` on **devnet** and test every action.
2. Paste the deployed **program id** into `PROGRAM_ID_STR` in `market.html` and
   `curve.html` (the fee wallet is already set).
3. Deploy to **mainnet** after testing.

## Security setup (do this before mainnet)

- **Run `supabase_schema.sql`** in the Supabase SQL editor. It creates the tables
  **and the Row-Level Security policies**. The site ships a public anon key — that
  is normal for Supabase, but without these policies anyone could read private DMs
  or write as another user. Also disable "Confirm email" (the app uses synthetic
  `username@h00d.fun` logins).
- **Domain-lock the Helius RPC key** to `h00d.fun` in the Helius dashboard. The key
  is embedded in the static pages (unavoidable for a no-backend site), so locking
  it to the site's origin is what stops quota theft.
- **Add SRI hashes** to the CDN `<script>` tags. Run `scripts/compute-sri.sh` from a
  networked machine and paste the printed `integrity="…"` attributes in. For the
  ESM/worker libraries (which can't use SRI), vendor them same-origin instead.
- **Set the program constants**: `declare_id!` and `FEE_RECIPIENT` in
  `program/lib.rs`, and `PROGRAM_ID_STR` in `market.html` / `curve.html`.

## Status & safety

This is an active build. The site pages are live; the on-chain market/curve are
**built and reviewed but not yet audited or deployed**. Do not route real funds
through the market/curve until the program has been tested on devnet and,
ideally, professionally audited.

## Tech

Static HTML + vanilla JS · `@solana/web3.js` · `@solana/spl-token` · Jupiter ·
Supabase (auth) · Anchor (program). Hosted on GitHub Pages (`CNAME`) / Railway.

## License

MIT — see [`LICENSE`](LICENSE).
