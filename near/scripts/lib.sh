# Helpers for the demo scripts.
source "$(dirname "${BASH_SOURCE[0]}")/env.sh"
RPC=${RPC:-https://test.rpc.fastnear.com}
# view <account> <method> [json]
view() {
  curl -s -X POST "$RPC" -H 'content-type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"query\",\"params\":{\"request_type\":\"call_function\",\"finality\":\"optimistic\",\"account_id\":\"$1\",\"method_name\":\"$2\",\"args_base64\":\"$(printf '%s' "${3:-{\}}" | base64 -w0)\"}}" |
    python3 -c "import sys,json;d=json.load(sys.stdin);r=d.get('result',{});print(bytes(r['result']).decode() if 'result' in r else (r.get('error') or d.get('error')))"
}
# call <signer> <contract> <method> <json> <deposit> [gas]
call() {
  near contract call-function as-transaction "$2" "$3" json-args "$4" prepaid-gas "${6:-100 Tgas}" \
    attached-deposit "$5" sign-as "$1" network-config "${NET:-testnet}" sign-with-legacy-keychain send 2>&1 |
    grep -E "Transaction ID|succeeded|Error|panicked|Smart contract|Logs \[" | sed 's/\x1b\[[0-9;]*m//g'
}
