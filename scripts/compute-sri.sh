#!/usr/bin/env bash
# Compute Subresource Integrity (SRI) hashes for the third-party scripts h00d.fun
# loads from CDNs. Run this from a machine WITH network access, then paste the
# printed integrity="…" attribute onto the matching <script> tag so a compromised
# CDN cannot silently swap the file (critical here: tweetnacl generates the mint
# keypair in launch.html, and supabase-js holds the session).
#
#   usage:  bash scripts/compute-sri.sh
#
# Note: importmap / ESM (esm.sh) and Web Worker importScripts() do NOT support SRI
# across browsers — for those (the @solana/* libs and the tweetnacl worker) the
# robust fix is to vendor the files into this repo and serve them same-origin.
set -euo pipefail

urls=(
  "https://cdnjs.cloudflare.com/ajax/libs/qrcodejs/1.0.0/qrcode.min.js"   # index.html <script>
  "https://cdn.jsdelivr.net/npm/@supabase/supabase-js@2"                  # index.html <script> (pin to an exact @2.x.y first)
  "https://cdn.jsdelivr.net/npm/tweetnacl@1.0.3/nacl.min.js"              # launch.html worker (vendor instead — see note)
)

for u in "${urls[@]}"; do
  h=$(curl -fsSL "$u" | openssl dgst -sha384 -binary | openssl base64 -A)
  echo "$u"
  echo "    integrity=\"sha384-$h\" crossorigin=\"anonymous\""
  echo
done
