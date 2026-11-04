# Testnet accounts of the demo (v1 by default; raids/env.sh switches to the Raids deployment). Keys live in ~/.near-credentials/testnet (near-cli legacy keychain), never here.
export PATH="$HOME/.cargo/bin:$PATH"
export FACTORY=sputnik-v2.testnet

export DAO=${DAO:-dao.warband-hq.testnet}     # Sputnik DAO v2 (release 3.0.0), the war chest
export HQ=warband-hq.testnet           # DAO council, funds the keeper
export TOKEN=${TOKEN:-warband-band.testnet}      # the Warband contract, locked after init
export ALICE=warband-alice.testnet
export BOB=warband-bob.testnet
export KEEPER=keeper.warband-hq.testnet
export WASM=${CARGO_TARGET_DIR:-$HOME/build/warband}/near/warband.wasm
