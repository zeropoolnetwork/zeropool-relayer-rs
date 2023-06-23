# zeropool-indexer-worker

Worker service responsible for indexing zeropool transactions from the blockchain.

## Environment variables

### Common
```bash
BACKEND=near-lake-framework
PG_URL="postgresql://postgres:postgres@localhost:5432/zeropool"
PG_MAX_CONNECTIONS=15
PG_RESET=false # if true, will drop all tables and recreate them
```

### EVM
```bash
EVM_CONTRACT_ADDRESS=0x0000000000000000000000000000000000000000 # the address of the pool contract
EVM_RPC_URL=
EVM_STARTING_BLOCK=0 # initial block height, omit to start from the beginning
EVM_REQUEST_INTERVAL=1000 # interval between requests to the node
```

### NEAR Indexer Framework
```bash
NEAR_CHAIN_ID="testnet"
NEAR_CONTRACT_ADDRESS=zeropool.testnet # the account id of the pool contract
NEAR_NODE_URL="https://rpc.testnet.near.org" # initial node
NEAR_BLOCK_HEIGHT=0 # initial block height
```

### NEAR Lake Framework
```bash
NEAR_CHAIN_ID="testnet"
NEAR_CONTRACT_ADDRESS=zeropool.testnet # the account id of the pool contract
NEAR_BLOCK_HEIGHT=0 # initial block height
AWS_ACCESS_KEY_ID=""
AWS_SECRET_ACCESS_KEY=""
```