#[allow(unused_imports)]
use b3_utils::api::{CallCycles, InterCall};
use b3_utils::{caller_is_controller, hex_string_with_0x_to_nat};
use b3_utils::{vec_to_hex_string_with_0x, Subaccount};
use candid::{CandidType, Deserialize, Nat};
use std::cell::RefCell;
use evm_rpc_canister_types::{
    EthSepoliaService, GetTransactionReceiptResult, MultiGetTransactionReceiptResult, RpcServices,
    EVM_RPC,
};
use rand::Rng;
use b3_utils::memory::init_stable_mem_refcell;
use b3_utils::memory::types::DefaultStableBTreeMap;
use b3_utils::ledger::{ICRCAccount, ICRC1, ICRC1TransferArgs, ICRC1TransferResult, ICRC2, ICRC2ApproveArgs, ICRC2ApproveResult};
use candid::Principal;

const MINTER_ADDRESS: &str = "0xb44b5e756a894775fc32eddf3314bb1b1944dc34";
const LEDGER: &str = "apia6-jaaaa-aaaar-qabma-cai";
const MINTER: &str = "jzenf-aiaaa-aaaar-qaa7q-cai";

thread_local! {
    static TRANSACTIONS: RefCell<DefaultStableBTreeMap<String, String>> = init_stable_mem_refcell("transactions", 1)
        .expect("Failed to initialize stable memory for transactions");
    static BALANCES: RefCell<DefaultStableBTreeMap<String, Nat>> = init_stable_mem_refcell("balances", 2)
        .expect("Failed to initialize stable memory for balances");
}

#[ic_cdk::query]
fn get_transaction_list() -> Vec<(String, String)> {
    TRANSACTIONS.with(|t| {
        t.borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    })
}

// Function to get a player's balance
#[ic_cdk::query]
fn get_balance(player: String) -> Nat {
    BALANCES.with(|b| {
        b.borrow()
            .get(&player)
            .cloned()
            .unwrap_or_else(|| Nat::from(0))
    })
}

// Function to spin the roulette wheel and determine the result
fn spin_roulette() -> u8 {
    let mut rng = rand::thread_rng();
    rng.gen_range(0..37) // Simulating a standard roulette with numbers from 0 to 36
}

#[derive(CandidType, Deserialize)]
pub struct VerifiedTransactionDetails {
    pub amount: Nat,
    pub from: String,
}

// Function for staking an amount to play the roulette
#[ic_cdk::update]
async fn play_roulette(player: String, hash: String, bet: BetType) -> Result<String, String> {
    // First check if the transaction has been processed
    if TRANSACTIONS.with(|t| t.borrow().contains_key(&hash)) {
        return Err("Transaction already processed".to_string());
    }

    // Verify the player's transaction
    let verified_details = verify_transaction(hash.clone()).await.map_err(|e| e.to_string())?;
    let VerifiedTransactionDetails { amount, from } = verified_details;

    if from != player {
        return Err("Player mismatch".to_string());
    }

    // Spin the roulette to get the winning number
    let winning_number = spin_roulette();

    // Determine the color of the winning number (Red/Black/Green)
    let winning_color = match winning_number {
        0 => "Green",
        1 | 3 | 5 | 7 | 9 | 12 | 14 | 16 | 18 | 19 | 21 | 23 | 25 | 27 | 30 | 32 | 34 | 36 => "Red",
        _ => "Black",
    };

    // Determine if the winning number is even or odd
    let even_odd = if winning_number % 2 == 0 && winning_number != 0 {
        "Even"
    } else {
        "Odd"
    };

    // Calculate the outcome based on the type of bet
    let result_message = match bet {
        BetType::Number(bet_number) => {
            if bet_number == winning_number {
                let winnings = amount.clone() * Nat::from(35); // Straight-up number bets pay 35:1
                update_player_balance(player.clone(), winnings);
                format!("Congratulations! You won with number {}. The winning number was {}.", bet_number, winning_number)
            } else {
                update_player_balance(player.clone(), -amount.clone());
                format!("You lost! The winning number was {}.", winning_number)
            }
        }
        BetType::Color(bet_color) => {
            if bet_color.to_lowercase() == winning_color.to_lowercase() {
                let winnings = amount.clone() * Nat::from(2); // Color bets pay 1:1
                update_player_balance(player.clone(), winnings);
                format!("Congratulations! You won with color {}. The winning color was {}.", bet_color, winning_color)
            } else {
                update_player_balance(player.clone(), -amount.clone());
                format!("You lost! The winning color was {}.", winning_color)
            }
        }
        BetType::EvenOdd(bet_even_odd) => {
            if bet_even_odd.to_lowercase() == even_odd.to_lowercase() {
                let winnings = amount.clone() * Nat::from(2); // Even/Odd bets pay 1:1
                update_player_balance(player.clone(), winnings);
                format!("Congratulations! You won with {}. The winning number was {} ({}).", bet_even_odd, winning_number, even_odd)
            } else {
                update_player_balance(player.clone(), -amount.clone());
                format!("You lost! The winning number was {} ({}).", winning_number, even_odd)
            }
        }
    };

    // Record the processed transaction
    TRANSACTIONS.with(|t| {
        let mut t = t.borrow_mut();
        t.insert(hash, from);
    });

    Ok(result_message)
}

fn update_player_balance(player: String, amount: Nat) {
    BALANCES.with(|b| {
        let mut balances = b.borrow_mut();
        let player_balance = balances.entry(player.clone()).or_insert(Nat::from(0));
        
        if amount < Nat::from(0) && *player_balance < amount.abs() {
            panic!("Insufficient balance");
        }

        *player_balance += amount;
    });
}

// Function for verifying the transaction on-chain
#[ic_cdk::update]
async fn verify_transaction(hash: String) -> Result<VerifiedTransactionDetails, String> {
    // Get the transaction receipt
    let receipt_result = eth_get_transaction_receipt(hash).await.map_err(|e| e.to_string())?;

    // Ensure the transaction was successful
    let receipt = match receipt_result {
        GetTransactionReceiptResult::Ok(Some(receipt)) => receipt,
        GetTransactionReceiptResult::Ok(None) => return Err("Receipt is None".to_string()),
        GetTransactionReceiptResult::Err(e) => return Err(format!("Error on Get transaction receipt result: {:?}", e)),
    };

    // Check if the status indicates success (Nat 1)
    let success_status = Nat::from(1u8);
    if receipt.status != success_status {
        return Err("Transaction failed".to_string());
    }

    // Verify the 'to' address matches the minter address
    if receipt.to != MINTER_ADDRESS {
        return Err("Minter address does not match".to_string());
    }

    let deposit_principal = canister_deposit_principal();

    // Verify the principal in the logs matches the deposit principal
    let log_principal = receipt
        .logs
        .iter()
        .find(|log| log.topics.get(2).map(|topic| topic.as_str()) == Some(&deposit_principal))
        .ok_or_else(|| "Principal not found in logs".to_string())?;

    // Extract relevant transaction details
    let amount = hex_string_with_0x_to_nat(&log_principal.data)
        .map_err(|e| format!("Failed to parse amount: {}", e))?;
    let from_address = receipt.from.clone();

    Ok(VerifiedTransactionDetails {
        amount,
        from: from_address,
    })
}

#[ic_cdk::query]
fn canister_deposit_principal() -> String {
    let subaccount = Subaccount::from(ic_cdk::id());
    let bytes32 = subaccount.to_bytes32().expect("Failed to convert to bytes32");
    vec_to_hex_string_with_0x(bytes32)
}

// Balance ---------------------------------
#[ic_cdk::update]
async fn balance() -> Nat {
    let account = ICRCAccount::new(ic_cdk::id(), None);
    ICRC1::from(LEDGER).balance_of(account).await.unwrap()
}

// Transfer --------------------------------
#[ic_cdk::update(guard = "caller_is_controller")]
async fn transfer(to: String, amount: Nat) -> ICRC1TransferResult {
    let to = ICRCAccount::from_str(&to).unwrap();
    let transfer_args = ICRC1TransferArgs {
        to,
        amount,
        from_subaccount: None,
        fee: None,
        memo: None,
        created_at_time: None,
    };

    ICRC1::from(LEDGER).transfer(transfer_args).await.unwrap()
}

// Approve ---------------------------------
#[ic_cdk::update(guard = "caller_is_controller")]
async fn approve(amount: Nat) -> ICRC2ApproveResult {
    let minter = Principal::from_text(&MINTER).unwrap();
    let spender = ICRCAccount::from(minter);

    let args = ICRC2ApproveArgs {
        amount,
        spender,
        created_at_time: None,
        expected_allowance: None,
        expires_at: None,
        fee: None,
        memo: None,
        from_subaccount: None,
    };

    ICRC2::from(LEDGER).approve(args).await.unwrap()
}

// Withdrawal ------------------------------
#[derive(CandidType, Deserialize)]
pub struct WithdrawalArg {
    pub amount: Nat,
    pub recipient: String,
}

#[derive(CandidType, Deserialize, Clone, Debug)]
pub struct RetrieveEthRequest {
    pub block_index: Nat,
}

#[derive(CandidType, Deserialize, Debug)]
pub enum WithdrawalError {
    AmountTooLow { min_withdrawal_amount: Nat },
    InsufficientFunds { balance: Nat },
    InsufficientAllowance { allowance: Nat },
    TemporarilyUnavailable(String),
}

type WithdrawalResult = Result<RetrieveEthRequest, WithdrawalError>;

#[ic_cdk::update(guard = "caller_is_controller")]
async fn withdraw(amount: Nat, recipient: String) -> WithdrawalResult {
    let withdraw = WithdrawalArg { amount, recipient };

    InterCall::from(MINTER)
        .call("withdraw_eth", withdraw, CallCycles::NoPay)
        .await
        .map_err(|e| WithdrawalError::TemporarilyUnavailable(e.to_string()))
}

// Export ---------------------------------
ic_cdk::export_candid!();
