# anchor/ — h00d_market Anchor workspace

Buildable/testable wrapper around the program. The actual program source is the
single file at the repo root: **`../program/lib.rs`** (this workspace builds it via
the `path = ...` line in `programs/h00d_market/Cargo.toml`, so there's no duplicate
copy to keep in sync).

```
anchor/
├── Anchor.toml                     # cluster + program id config
├── Cargo.toml                      # workspace (overflow-checks on)
├── package.json                    # JS test deps
├── tsconfig.json
├── programs/h00d_market/
│   └── Cargo.toml                  # -> builds ../../../program/lib.rs
├── tests/h00d_market.ts            # curve + market + guard tests
└── migrations/deploy.ts
```

## Use

```bash
cd anchor
npm install
anchor test          # local validator + the test suite
anchor build         # build only
```

Deploying (devnet/mainnet) is covered in **`../DEPLOY.md`**.

The tests skip `withdraw_graduated` (it needs the `FEE_RECIPIENT` private key, a
fixed address in the program) — exercise that one on devnet with that wallet.
