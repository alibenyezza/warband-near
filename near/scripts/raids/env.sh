# The Raids deployment (v2). The v1 contract (warband-band.testnet) is locked for good, and its DAO's staking
# contract cannot change, so v2 gets a new contract account and a new DAO with the same council and policy.
export TOKEN=warband-raids.testnet
export DAO=dao-raids.warband-hq.testnet
export CAMP=warband-camp.testnet        # funds the demo raiders, one sub-account each
export DAY_SECONDS=${DAY_SECONDS:-180} # one raid every 3 minutes on testnet; 86400 for a real launch
export NET=testnet-fastnear     # near-cli network; the default archival RPC lags and lets transactions expire
export RAIDERS="archer bard drummer hunter mason monk piper rider scout smith tanner warden"
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
raider() { echo "$1.$CAMP"; }
