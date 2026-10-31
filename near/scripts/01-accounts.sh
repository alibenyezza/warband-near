#!/usr/bin/env bash
# Creates the demo accounts with the near-cli faucet (10 testnet NEAR each). Keys go to ~/.near-credentials/testnet.
# The faucet is rate-limited: wait a minute between accounts if it refuses.
set -euo pipefail
source "$(dirname "$0")/env.sh"
for a in "$HQ" "$TOKEN" "$ALICE" "$BOB"; do
  near account create-account sponsor-by-faucet-service "$a" autogenerate-new-keypair \
    save-to-legacy-keychain network-config testnet create
done
# The keeper is a sub-account of HQ: the human keeps its full-access key, the agent only gets a function-call key.
near account create-account fund-myself "$KEEPER" '0.5 NEAR' autogenerate-new-keypair save-to-legacy-keychain \
  sign-as "$HQ" network-config testnet sign-with-legacy-keychain send
