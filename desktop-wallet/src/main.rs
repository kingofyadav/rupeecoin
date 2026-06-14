#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod keystore;
mod wallet_commands;

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            wallet_commands::generate_wallet,
            wallet_commands::unlock_wallet,
            wallet_commands::get_height,
            wallet_commands::get_balance,
            wallet_commands::get_nonce,
            wallet_commands::send_transaction,
            wallet_commands::advance_address,
        ])
        .run(tauri::generate_context!())
        .expect("error while running RupeeCoin wallet");
}
