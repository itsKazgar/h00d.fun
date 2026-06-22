# Go-live checklist — h00d.fun

Work top to bottom. Anything under **Hard gates** must be done before real SOL
touches the program. Items marked _(done in code)_ are already in this repo and
just need to ship; items marked _(you)_ happen in your own dashboards/wallets and
can't be done from the codebase.

## Hard gates (fund-holding program — do not skip)

- [ ] **Build it.** Scaffold the Anchor workspace (`Cargo.toml`, `Anchor.toml`,
      `tests/`) so `program/lib.rs` actually compiles and deploys. _(you — you said
      you'd handle Anchor)_
- [ ] **Test it.** Write a suite covering buy/sell math, graduation, slippage, the
      floor ratchet, escrow, cancel, and the failure cases (overflow, draining,
      wrong-account, self-trade). Wire it into CI (`.github/workflows/ci.yml`
      already has the job, gated on `Anchor.toml`).
- [ ] **Audit it.** Independent review by a Solana/Anchor auditor once tests pass
      and the build is verifiable. _(you)_
- [ ] **Verifiable build + upgrade authority.** Publish with `solana-verify` so the
      on-chain bytecode matches this source, and move the program's upgrade
      authority to a multisig (e.g. Squads). _(you)_

## On-chain configuration

- [ ] Set `declare_id!(...)` in `program/lib.rs` to your deployed program id.
- [ ] Set `FEE_RECIPIENT` in `program/lib.rs` to your fee wallet.
- [ ] Review economics: `INITIAL_VIRTUAL_SOL`, `GRADUATION_LAMPORTS`, `FEE_BPS`.
- [ ] Decide on the graduation → Raydium LP path (today `withdraw_graduated` only
      pulls SOL; leftover curve tokens stay in the vault).

## Frontend configuration

- [ ] Paste the program id into `PROGRAM_ID_STR` in `market.html` and `curve.html`.
- [ ] Confirm `FEE_RECIPIENT_STR` (both pages) matches the program's `FEE_RECIPIENT`.
- [ ] Set the swap referral `FEE_ACCOUNT` (swap.html) and launch `FEE_WALLET`
      (launch.html) if you want those fees, or leave blank to disable.
- [x] _(done in code)_ Priority fees on all program transactions so they land on a
      busy mainnet.
- [ ] Consider a backup RPC and Wallet-Standard / wallet-adapter support (today only
      Phantom/Solflare are detected).

## Security (mostly your dashboards)

- [ ] **Run `supabase_schema.sql`** in the Supabase SQL editor — creates the RLS
      policies. Until then, the public anon key exposes DMs/posts. _(you)_ _(schema
      done in code)_
- [ ] Disable "Confirm email" in Supabase auth (the app uses `username@h00d.fun`).
- [ ] **Domain-lock the Helius RPC key** to `h00d.fun`, and rotate it (it's
      committed). The key is in `swap/launch/curve/market/index.html`. _(you)_
- [ ] **Add SRI hashes** to the CDN `<script>` tags: run `scripts/compute-sri.sh`
      from a networked machine and paste the `integrity="…"` attributes. _(you)_
- [x] _(done in code)_ XSS fix in swap.html token search.

## Ops & polish

- [ ] Token metadata/image hosting (IPFS/Arweave) — launch only stores a URI.
- [ ] Terms of service + risk disclaimer; moderation for the feed/DMs.
- [ ] Error monitoring + Helius usage alerts.
- [ ] Deploy to **devnet** first, exercise every action end-to-end, then mainnet.
