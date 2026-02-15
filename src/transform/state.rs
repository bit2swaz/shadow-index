use crate::db::models::StorageDiffRow;
use revm::database::BundleState;

pub fn transform_state(bundle: &BundleState, block_number: u64, sign: i8) -> Vec<StorageDiffRow> {
    let mut storage_diffs = Vec::new();

    for (address, account) in bundle.state.iter() {
        if account.storage.is_empty() {
            continue;
        }

        let address_bytes = address.as_slice().to_vec();

        for (slot, storage_value) in account.storage.iter() {
            let slot_bytes = slot.to_be_bytes::<32>().to_vec();

            let value_bytes = storage_value.present_value.to_be_bytes::<32>().to_vec();

            storage_diffs.push(StorageDiffRow {
                block_number,
                sign,
                address: address_bytes.clone(),
                slot: slot_bytes,
                value: value_bytes,
            });
        }
    }

    storage_diffs
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, U256};
    use revm::database::BundleAccount;
    use revm::state::AccountInfo;
    use revm_database::states::{AccountStatus, StorageSlot};
    use std::collections::HashMap;

    #[test]
    fn test_transform_state_empty_bundle() {
        let bundle = BundleState::default();
        let diffs = transform_state(&bundle, 100, 1);
        assert_eq!(diffs.len(), 0, "empty bundle should produce no diffs");
    }

    #[test]
    fn test_transform_state_single_storage_change() {
        let mut bundle = BundleState::default();

        let address = Address::repeat_byte(0x12);
        let slot = U256::from(0xabc);
        let value = U256::from(0x999);

        let mut storage = HashMap::default();
        storage.insert(
            slot,
            StorageSlot {
                previous_or_original_value: U256::ZERO,
                present_value: value,
            },
        );

        let account = BundleAccount {
            info: None,
            original_info: None,
            storage,
            status: AccountStatus::default(),
        };

        bundle.state.insert(address, account);

        let diffs = transform_state(&bundle, 100, 1);

        assert_eq!(diffs.len(), 1, "should have exactly one storage diff");

        let diff = &diffs[0];
        assert_eq!(diff.block_number, 100);
        assert_eq!(diff.sign, 1);
        assert_eq!(diff.address, address.as_slice().to_vec());
        assert_eq!(diff.slot, slot.to_be_bytes::<32>().to_vec());
        assert_eq!(diff.value, value.to_be_bytes::<32>().to_vec());
    }

    #[test]
    fn test_transform_state_multiple_slots_single_account() {
        let mut bundle = BundleState::default();

        let address = Address::repeat_byte(0x42);

        let mut storage = HashMap::default();
        for i in 1..=5 {
            storage.insert(
                U256::from(i),
                StorageSlot {
                    previous_or_original_value: U256::ZERO,
                    present_value: U256::from(i * 10),
                },
            );
        }

        let account = BundleAccount {
            info: None,
            original_info: None,
            storage,
            status: AccountStatus::default(),
        };

        bundle.state.insert(address, account);

        let diffs = transform_state(&bundle, 200, 1);

        assert_eq!(diffs.len(), 5, "should have 5 storage diffs");

        for diff in &diffs {
            assert_eq!(diff.block_number, 200);
            assert_eq!(diff.sign, 1);
            assert_eq!(diff.address, address.as_slice().to_vec());
        }
    }

    #[test]
    fn test_transform_state_multiple_accounts() {
        let mut bundle = BundleState::default();

        let addr1 = Address::repeat_byte(0x01);
        let mut storage1 = HashMap::default();
        storage1.insert(
            U256::from(100),
            StorageSlot {
                previous_or_original_value: U256::ZERO,
                present_value: U256::from(500),
            },
        );

        bundle.state.insert(
            addr1,
            BundleAccount {
                info: None,
                original_info: None,
                storage: storage1,
                status: AccountStatus::default(),
            },
        );

        let addr2 = Address::repeat_byte(0x02);
        let mut storage2 = HashMap::default();
        storage2.insert(
            U256::from(200),
            StorageSlot {
                previous_or_original_value: U256::ZERO,
                present_value: U256::from(600),
            },
        );

        bundle.state.insert(
            addr2,
            BundleAccount {
                info: None,
                original_info: None,
                storage: storage2,
                status: AccountStatus::default(),
            },
        );

        let diffs = transform_state(&bundle, 300, 1);

        assert_eq!(
            diffs.len(),
            2,
            "should have 2 storage diffs (one per account)"
        );

        let addresses: Vec<Vec<u8>> = diffs.iter().map(|d| d.address.clone()).collect();
        assert!(addresses.contains(&addr1.as_slice().to_vec()));
        assert!(addresses.contains(&addr2.as_slice().to_vec()));
    }

    #[test]
    fn test_transform_state_with_revert_sign() {
        let mut bundle = BundleState::default();

        let address = Address::repeat_byte(0xff);
        let mut storage = HashMap::default();
        storage.insert(
            U256::from(1),
            StorageSlot {
                previous_or_original_value: U256::ZERO,
                present_value: U256::from(100),
            },
        );

        bundle.state.insert(
            address,
            BundleAccount {
                info: None,
                original_info: None,
                storage,
                status: AccountStatus::default(),
            },
        );

        let diffs = transform_state(&bundle, 500, -1);

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].sign, -1, "should have negative sign for revert");
        assert_eq!(diffs[0].block_number, 500);
    }

    #[test]
    fn test_transform_state_skips_accounts_without_storage() {
        let mut bundle = BundleState::default();

        let addr1 = Address::repeat_byte(0x01);
        let mut storage1 = HashMap::default();
        storage1.insert(
            U256::from(1),
            StorageSlot {
                previous_or_original_value: U256::ZERO,
                present_value: U256::from(100),
            },
        );

        bundle.state.insert(
            addr1,
            BundleAccount {
                info: Some(AccountInfo {
                    balance: U256::from(1000),
                    nonce: 1,
                    code_hash: alloy_primitives::B256::ZERO,
                    code: None,
                    account_id: None,
                }),
                original_info: None,
                storage: storage1,
                status: AccountStatus::default(),
            },
        );

        let addr2 = Address::repeat_byte(0x02);
        bundle.state.insert(
            addr2,
            BundleAccount {
                info: Some(AccountInfo {
                    balance: U256::from(2000),
                    nonce: 5,
                    code_hash: alloy_primitives::B256::ZERO,
                    code: None,
                    account_id: None,
                }),
                original_info: None,
                storage: HashMap::default(),
                status: AccountStatus::default(),
            },
        );

        let diffs = transform_state(&bundle, 400, 1);

        assert_eq!(
            diffs.len(),
            1,
            "should only have 1 diff from account with storage changes"
        );
        assert_eq!(diffs[0].address, addr1.as_slice().to_vec());
    }

    #[test]
    fn test_transform_state_large_values() {
        let mut bundle = BundleState::default();

        let address = Address::repeat_byte(0xaa);
        let mut storage = HashMap::default();

        let max_slot = U256::MAX;
        let max_value = U256::MAX;

        storage.insert(
            max_slot,
            StorageSlot {
                previous_or_original_value: U256::ZERO,
                present_value: max_value,
            },
        );

        bundle.state.insert(
            address,
            BundleAccount {
                info: None,
                original_info: None,
                storage,
                status: AccountStatus::default(),
            },
        );

        let diffs = transform_state(&bundle, 1000, 1);

        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].slot.len(), 32, "slot should be 32 bytes");
        assert_eq!(diffs[0].value.len(), 32, "value should be 32 bytes");
        assert_eq!(diffs[0].value, max_value.to_be_bytes::<32>().to_vec());
    }
}
