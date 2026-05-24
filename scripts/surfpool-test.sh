#!/bin/bash
set -e

PROGRAM_NAME=roshi
TEST_KEYPAIR_PATH=./target/deploy/roshi-keypair.json
DEPLOYER_WALLET=~/.config/solana/id.json
AIRDROP_AMOUNT=100

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

SURFPOOL_PID=""
SCRIPT_EXIT_CODE=1

cleanup() {
  echo -e "\n${YELLOW}Stopping Surfpool...${NC}"

  if [ "${CI:-false}" = "true" ]; then
    echo -e "${YELLOW}Running in CI environment - skipping manual cleanup${NC}"
    exit $SCRIPT_EXIT_CODE
  fi

  pkill -9 -f "surfpool" 2>/dev/null || true
  sleep 2

  if pgrep -f "surfpool" > /dev/null 2>&1; then
    echo -e "${YELLOW}Force killing remaining processes...${NC}"
    pkill -9 -f "surfpool" 2>/dev/null || true
    sleep 1
  fi

  if pgrep -f "surfpool" > /dev/null 2>&1; then
    echo -e "${RED}Warning: some Surfpool processes may still be running${NC}"
  else
    echo -e "${GREEN}Surfpool stopped${NC}"
  fi

  exit $SCRIPT_EXIT_CODE
}

trap cleanup EXIT INT TERM

echo -e "${GREEN}=== Building Program ===${NC}"

mkdir -p target/deploy

if [ ! -f "$TEST_KEYPAIR_PATH" ]; then
  echo -e "${YELLOW}Generating program keypair...${NC}"
  solana-keygen new --no-bip39-passphrase -o "$TEST_KEYPAIR_PATH"
fi

echo -e "${YELLOW}Using test keypair: $TEST_KEYPAIR_PATH${NC}"
TEST_PROGRAM_ID=$(solana-keygen pubkey "$TEST_KEYPAIR_PATH")
echo -e "${YELLOW}Program ID: $TEST_PROGRAM_ID${NC}"

cargo build-sbf --manifest-path crates/roshi/Cargo.toml
echo -e "${GREEN}Program built${NC}\n"

echo -e "${GREEN}=== Starting Surfpool ===${NC}"

mkdir -p .surfpool
surfpool start --no-tui --yes > .surfpool/surfpool.log 2>&1 &
SURFPOOL_PID=$!

echo -e "${GREEN}Surfpool started (PID: $SURFPOOL_PID)${NC}"
echo -e "${YELLOW}Waiting for Surfpool RPC to be ready...${NC}"

MAX_RETRIES=30
RETRY_COUNT=0
while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
  if curl -s -X POST http://127.0.0.1:8899 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}' \
    > /dev/null 2>&1; then
    echo -e "${GREEN}Surfpool RPC is up${NC}"
    break
  fi

  RETRY_COUNT=$((RETRY_COUNT + 1))
  if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    echo -e "${RED}Surfpool RPC failed to start within 30 seconds${NC}"
    echo -e "${RED}Check .surfpool/surfpool.log for details${NC}"
    exit 1
  fi

  sleep 1
done

echo -e "${YELLOW}Waiting for Surfpool to complete deployment...${NC}"
sleep 5

PROGRAM_ID="$TEST_PROGRAM_ID"
echo -e "${YELLOW}Verifying program deployment: $PROGRAM_ID${NC}"

PROGRAM_INFO=$(curl -s -X POST http://127.0.0.1:8899 \
  -H "Content-Type: application/json" \
  -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccountInfo\",\"params\":[\"$PROGRAM_ID\",{\"encoding\":\"base64\"}]}")

NEEDS_DEPLOY=false
if echo "$PROGRAM_INFO" | grep -q '"value":null'; then
  echo -e "${YELLOW}Program account does not exist; falling back to manual deploy${NC}"
  NEEDS_DEPLOY=true
elif ! echo "$PROGRAM_INFO" | grep -q '"executable":true'; then
  echo -e "${YELLOW}Program account exists but is not executable; falling back to manual deploy${NC}"
  NEEDS_DEPLOY=true
else
  echo -e "${GREEN}Program deployed successfully by Surfpool${NC}"
fi

if [ "$NEEDS_DEPLOY" = true ]; then
  echo -e "${YELLOW}Last few lines of Surfpool log:${NC}"
  tail -n 10 .surfpool/surfpool.log
  echo ""

  DEPLOYER_PUBKEY=$(solana-keygen pubkey "$DEPLOYER_WALLET")
  solana airdrop "$AIRDROP_AMOUNT" "$DEPLOYER_PUBKEY" --url http://127.0.0.1:8899

  echo -e "${YELLOW}Writing program buffer...${NC}"
  BUFFER_ACCOUNT=$(solana program write-buffer \
    "target/deploy/${PROGRAM_NAME}.so" \
    --url http://127.0.0.1:8899 \
    --keypair "$DEPLOYER_WALLET" \
    --with-compute-unit-price 1000 2>&1 | grep "Buffer:" | awk '{print $2}')

  if [ -z "$BUFFER_ACCOUNT" ]; then
    echo -e "${RED}Failed to create buffer account${NC}"
    exit 1
  fi

  echo -e "${YELLOW}Deploying from buffer...${NC}"
  solana program deploy \
    --program-id "$TEST_KEYPAIR_PATH" \
    --buffer "$BUFFER_ACCOUNT" \
    --upgrade-authority "$DEPLOYER_WALLET" \
    --url http://127.0.0.1:8899 \
    --keypair "$DEPLOYER_WALLET"

  sleep 2
  PROGRAM_INFO_AFTER=$(curl -s -X POST http://127.0.0.1:8899 \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"getAccountInfo\",\"params\":[\"$PROGRAM_ID\",{\"encoding\":\"base64\"}]}")

  if echo "$PROGRAM_INFO_AFTER" | grep -q '"value":null'; then
    echo -e "${RED}Program still does not exist after deploy${NC}"
    exit 1
  fi

  if ! echo "$PROGRAM_INFO_AFTER" | grep -q '"executable":true'; then
    echo -e "${RED}Program still not executable after deploy${NC}"
    exit 1
  fi

  echo -e "${GREEN}Program deployed at: $PROGRAM_ID${NC}"
fi

echo -e "${GREEN}Surfpool is ready${NC}\n"
echo -e "${GREEN}=== Running Tests ===${NC}\n"

set +e
RPC_URL="http://127.0.0.1:8899" cargo test -p roshi-tests -- --ignored --nocapture
SCRIPT_EXIT_CODE=$?
set -e

if [ $SCRIPT_EXIT_CODE -eq 0 ]; then
  echo -e "\n${GREEN}=== All tests passed ===${NC}"
else
  echo -e "\n${RED}=== Tests failed with exit code $SCRIPT_EXIT_CODE ===${NC}"
fi
