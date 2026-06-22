#!/usr/bin/env bash
# Syntax-check the inline JavaScript in the static pages. There's no bundler, so
# this catches parse errors before they ship. Used by CI and runnable locally:
#   bash scripts/check-web.sh
set -uo pipefail
fail=0

check_module(){ # extract <script type="module"> body and parse as ESM
  awk '/<script type="module">/{f=1;next} /<\/script>/{if(f)f=0} f' "$1" > /tmp/_mod.mjs
  if [ -s /tmp/_mod.mjs ]; then
    if node --check /tmp/_mod.mjs 2>/tmp/_err; then echo "ok    $1 (module)"
    else echo "FAIL  $1 (module)"; cat /tmp/_err; fail=1; fi
  fi
}

for f in swap.html launch.html market.html curve.html; do
  [ -f "$f" ] && check_module "$f"
done

# index.html main app: the <script> block that defines SB_URL
awk '/<script>/{f=1;buf="";next} /<\/script>/{if(f){if(buf ~ /SB_URL/) print buf > "/tmp/_idx.js"; f=0} next} f{buf=buf $0 "\n"}' index.html
if [ -s /tmp/_idx.js ]; then
  if node --check /tmp/_idx.js 2>/tmp/_err; then echo "ok    index.html (app)"
  else echo "FAIL  index.html (app)"; cat /tmp/_err; fail=1; fi
fi

[ "$fail" = "0" ] && echo "all page scripts parse ✓" || echo "some scripts failed to parse ✗"
exit $fail
