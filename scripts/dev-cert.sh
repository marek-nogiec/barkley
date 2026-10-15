#!/bin/sh
# One-time: create a self-signed "Barkley Dev" code-signing identity in its own
# keychain, so every rebuild has the same signature and macOS keeps the
# Accessibility grant. Your login keychain and trust settings are not touched.
set -eu

name="Barkley Dev"
keychain="$HOME/Library/Keychains/barkley-dev.keychain-db"

if [ -f "$keychain" ] && security find-certificate -c "$name" "$keychain" >/dev/null 2>&1; then
  echo "already set up: $keychain"
  exit 0
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/cert.cnf" <<CNF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF

openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$tmp/cert.cnf" \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
# SHA1/3DES PKCS#12: the only flavour `security import` reads reliably.
openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -name "$name" \
  -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES -macalg sha1 \
  -passout pass:barkley -out "$tmp/id.p12"

# Dev-only keychain with an empty password; it holds nothing but this key.
[ -f "$keychain" ] || security create-keychain -p "" "$keychain"
security set-keychain-settings "$keychain"   # no auto-lock
security unlock-keychain -p "" "$keychain"
security import "$tmp/id.p12" -k "$keychain" -P barkley -T /usr/bin/codesign >/dev/null
# Let codesign use the key without a GUI prompt.
security set-key-partition-list -S apple-tool:,apple: -s -k "" "$keychain" >/dev/null

echo "created \"$name\" in $keychain"
