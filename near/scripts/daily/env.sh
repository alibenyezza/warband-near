# The daily deployment (v3): the same Warband contract as v2, with launch parameters (one raid per real day).
# v1 (warband-band.testnet) and v2 (warband-raids.testnet, 3-minute days) stay as the recorded fast runs.
export TOKEN=warband-daily.testnet
export DAO=dao-daily.warband-hq.testnet
export BOARD=warband-dailyboard.testnet
export GAS_KEEPER=gk-daily.warband-hq.testnet
export CAMP=warband-camp.testnet       # funds the twelve demo raiders, one sub-account each (shared with v2)
export DAY_SECONDS=86400
export NET=testnet-fastnear
export RAIDERS="archer bard drummer hunter mason monk piper rider scout smith tanner warden"
source "$(dirname "${BASH_SOURCE[0]}")/../lib.sh"
raider() { echo "$1.$CAMP"; }
