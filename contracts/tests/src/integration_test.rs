#[cfg(test)]
mod tests {
    use credit_oracle::{CreditOracle, CreditOracleClient, ScoringWeights, TxStats};
    use governance::{Governance, GovernanceClient, GovernanceError};
    use identity_oracle::{IdentityOracle, IdentityOracleClient};
    use revocation_registry::{RevocationRegistry, RevocationRegistryClient};
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger as _},
        BytesN, Env, String, Val,
    };

    #[test]
    fn test_initialize_emits_init_event() {
        let env = Env::default();
        env.mock_all_auths();

        let identity_id = env.register_contract(None, IdentityOracle);
        let credit_id = env.register_contract(None, CreditOracle);
        let revocation_id = env.register_contract(None, RevocationRegistry);

        let identity = IdentityOracleClient::new(&env, &identity_id);
        let credit = CreditOracleClient::new(&env, &credit_id);
        let revocation = RevocationRegistryClient::new(&env, &revocation_id);

        let admin = soroban_sdk::Address::generate(&env);

        // Initialize identity-oracle and verify Init event
        identity.initialize(&admin);
        let events = env.events().all();
        let id_events: Vec<_> = events.iter().filter(|(id, _, _)| *id == identity_id).collect();
        assert_eq!(id_events.len(), 1, "identity-oracle should emit 1 event");
        let (_, topics, data) = &id_events[0];
        assert_eq!(topics.len(), 1);
        assert_eq!(topics.get(0).unwrap(), soroban_sdk::Val::from(symbol_short!("Init")));
        let event_admin: soroban_sdk::Address = data.clone().unwrap();
        assert_eq!(event_admin, admin, "Init event admin mismatch for identity-oracle");

        // Initialize credit-oracle and verify Init event
        credit.initialize(&admin);
        let events = env.events().all();
        let credit_events: Vec<_> = events.iter().filter(|(id, _, _)| *id == credit_id).collect();
        assert_eq!(credit_events.len(), 1, "credit-oracle should emit 1 event");
        let (_, topics, data) = &credit_events[0];
        assert_eq!(topics.len(), 1);
        assert_eq!(topics.get(0).unwrap(), soroban_sdk::Val::from(symbol_short!("Init")));
        let event_admin: soroban_sdk::Address = data.clone().unwrap();
        assert_eq!(event_admin, admin, "Init event admin mismatch for credit-oracle");

        // Initialize revocation-registry and verify Init event
        revocation.initialize(&admin);
        let events = env.events().all();
        let rev_events: Vec<_> = events.iter().filter(|(id, _, _)| *id == revocation_id).collect();
        assert_eq!(rev_events.len(), 1, "revocation-registry should emit 1 event");
        let (_, topics, data) = &rev_events[0];
        assert_eq!(topics.len(), 1);
        assert_eq!(topics.get(0).unwrap(), soroban_sdk::Val::from(symbol_short!("Init")));
        let event_admin: soroban_sdk::Address = data.clone().unwrap();
        assert_eq!(event_admin, admin, "Init event admin mismatch for revocation-registry");
    }

    #[test]
    fn test_full_protocol_flow() {
        // 1. Create Env with mock_all_auths
        let env = Env::default();
        env.mock_all_auths();

        // 2. Register and initialize all 3 contracts
        let identity_id = env.register_contract(None, IdentityOracle);
        let credit_id = env.register_contract(None, CreditOracle);
        let _revocation_id = env.register_contract(None, RevocationRegistry);

        let identity = IdentityOracleClient::new(&env, &identity_id);
        let credit = CreditOracleClient::new(&env, &credit_id);
        let revocation = RevocationRegistryClient::new(&env, &_revocation_id);

        let admin = soroban_sdk::Address::generate(&env);
        identity.initialize(&admin);
        credit.initialize(&admin);
        revocation.initialize(&admin);

        // 3. Register an issuer in identity-oracle
        let issuer = soroban_sdk::Address::generate(&env);
        identity.register_issuer(&issuer);

        // 4. Call anchor_did for a test subject
        let subject = soroban_sdk::Address::generate(&env);
        let cid = String::from_str(&env, "ipfs://QmTestDID");
        identity.anchor_did(&subject, &cid);

        // 5. Call anchor_vc for the subject with a test hash
        let vc_hash = BytesN::from_array(&env, &[42u8; 32]);
        identity.anchor_vc(&issuer, &subject, &vc_hash);

        // 6. Assert is_verified returns true
        assert!(identity.is_verified(&subject));

        // 7. Register a lender and feeder in credit-oracle
        let lender = soroban_sdk::Address::generate(&env);
        let feeder = soroban_sdk::Address::generate(&env);
        credit.register_lender(&lender);
        credit.register_feeder(&feeder);

        // 8. Call set_vc_count(subject, 1)
        credit.set_vc_count(&feeder, &subject, &1);

        // 9. Call update_tx_stats with volume_30d = 500_000_000 stroops
        credit.update_tx_stats(
            &feeder,
            &subject,
            &TxStats {
                volume_30d: 500_000_000i128,
                tx_count_30d: 10,
                avg_counterparties: 3,
            },
        );

        // 10. Call record_repayment 5 times on_time=true
        for _ in 0..5 {
            credit.record_repayment(&lender, &subject, &100_000_000i128, &true);
        }

        // 11. Call compute_score
        let score = credit.compute_score(&subject);

        // 12. Assert score > 300
        assert!(score > 300, "expected score > 300, got {}", score);

        // 13. Assert score <= 850
        assert!(score <= 850, "expected score <= 850, got {}", score);
    }

    #[test]
    fn test_cross_contract_vc_count() {
        let env = Env::default();
        env.mock_all_auths();

        let identity_id = env.register_contract(None, IdentityOracle);
        let credit_id = env.register_contract(None, CreditOracle);

        let identity = IdentityOracleClient::new(&env, &identity_id);
        let credit = CreditOracleClient::new(&env, &credit_id);

        let admin = soroban_sdk::Address::generate(&env);
        identity.initialize(&admin);
        credit.initialize(&admin);

        let issuer = soroban_sdk::Address::generate(&env);
        identity.register_issuer(&issuer);

        let subject = soroban_sdk::Address::generate(&env);
        let cid = String::from_str(&env, "ipfs://QmTestDID");
        identity.anchor_did(&subject, &cid);

        let vc_hash = BytesN::from_array(&env, &[7u8; 32]);
        identity.anchor_vc(&issuer, &subject, &vc_hash);

        // Configure credit-oracle to call identity-oracle directly
        credit.set_identity_oracle(&identity_id);

        // Do not set cached VcCount; compute_score should read identity-oracle
        let score_live = credit.compute_score(&subject);
        assert!(
            score_live > 300,
            "expected live score > 300, got {}",
            score_live
        );

        // Now set the cached value to 0 to ensure the cross-contract path is used
        let feeder = soroban_sdk::Address::generate(&env);
        credit.register_feeder(&feeder);
        credit.set_vc_count(&feeder, &subject, &0);
        env.ledger()
            .set_sequence_number(env.ledger().sequence() + 1);
        let score_after_cached_zero = credit.compute_score(&subject);
        assert_eq!(
            score_live, score_after_cached_zero,
            "expected compute_score to prefer identity-oracle over cached VcCount"
        );
    }

    #[test]
    fn test_revoked_vc_lowers_score() {
        let env = Env::default();
        env.mock_all_auths();

        // Setup: register and initialize all 3 contracts
        let identity_id = env.register_contract(None, IdentityOracle);
        let credit_id = env.register_contract(None, CreditOracle);
        let revocation_id = env.register_contract(None, RevocationRegistry);

        let identity = IdentityOracleClient::new(&env, &identity_id);
        let credit = CreditOracleClient::new(&env, &credit_id);
        let _revocation = RevocationRegistryClient::new(&env, &revocation_id);

        let admin = soroban_sdk::Address::generate(&env);
        identity.initialize(&admin);
        credit.initialize(&admin);
        _revocation.initialize(&admin);

        let issuer = soroban_sdk::Address::generate(&env);
        identity.register_issuer(&issuer);

        let subject = soroban_sdk::Address::generate(&env);
        let cid = String::from_str(&env, "ipfs://QmTestDID");
        identity.anchor_did(&subject, &cid);

        let vc_hash = BytesN::from_array(&env, &[99u8; 32]);
        identity.anchor_vc(&issuer, &subject, &vc_hash);

        let lender = soroban_sdk::Address::generate(&env);
        let feeder = soroban_sdk::Address::generate(&env);
        credit.register_lender(&lender);
        credit.register_feeder(&feeder);

        // 1. Get initial score with vc_count = 1
        credit.set_vc_count(&feeder, &subject, &1);
        credit.update_tx_stats(
            &feeder,
            &subject,
            &TxStats {
                volume_30d: 500_000_000i128,
                tx_count_30d: 10,
                avg_counterparties: 3,
            },
        );
        for _ in 0..5 {
            credit.record_repayment(&lender, &subject, &100_000_000i128, &true);
        }
        let initial_score = credit.compute_score(&subject);
        assert!(initial_score > 300, "expected initial_score > 300, got {}", initial_score);

        // 2. Revoke the VC on identity-oracle
        identity.mark_vc_revoked(&issuer, &subject, &vc_hash);

        // 3. Assert is_verified returns false
        assert!(!identity.is_verified(&subject));

        // 4. Update vc_count to 0 and recompute score
        credit.set_vc_count(&feeder, &subject, &0);
        env.ledger()
            .set_sequence_number(env.ledger().sequence() + 1);
        let new_score = credit.compute_score(&subject);

        // 5. Assert new score < initial score
        assert!(
            new_score < initial_score,
            "expected new_score ({}) < initial_score ({})",
            new_score,
            initial_score
        );
    }

    #[test]
    fn test_revocation_registry_identity_oracle_integration() {
        let env = Env::default();
        env.mock_all_auths();

        let identity_id = env.register_contract(None, IdentityOracle);
        let revocation_id = env.register_contract(None, RevocationRegistry);

        let identity = IdentityOracleClient::new(&env, &identity_id);
        let revocation = RevocationRegistryClient::new(&env, &revocation_id);

        let admin = soroban_sdk::Address::generate(&env);
        identity.initialize(&admin);
        revocation.initialize(&admin);

        // Link identity-oracle to revocation-registry
        identity.set_revocation_registry(&revocation_id);

        let issuer = soroban_sdk::Address::generate(&env);
        identity.register_issuer(&issuer);

        let subject = soroban_sdk::Address::generate(&env);
        let vc_hash = BytesN::from_array(&env, &[123u8; 32]);
        identity.anchor_vc(&issuer, &subject, &vc_hash);

        // Assert verified initially
        assert!(identity.is_verified(&subject));

        // Revoke via revocation-registry
        revocation.revoke(&issuer, &vc_hash);

        // Verify that is_revoked returns true on the registry
        assert!(revocation.is_revoked(&vc_hash));

        // Verify that identity-oracle verify_vc returns false
        assert!(!identity.verify_vc(&subject, &vc_hash));

        // Also verify that is_verified and get_active_vc_count correctly reflect the revocation
        assert!(!identity.is_verified(&subject));
        assert_eq!(identity.get_active_vc_count(&subject), 0);
    }

    #[test]
    fn test_only_registered_issuer_can_revoke_vc_hash_integration() {
        let env = Env::default();
        env.mock_all_auths();

        // Setup: register and initialize all 3 contracts
        let identity_id = env.register_contract(None, IdentityOracle);
        let credit_id = env.register_contract(None, CreditOracle);
        let revocation_id = env.register_contract(None, RevocationRegistry);

        let _identity = IdentityOracleClient::new(&env, &identity_id);
        let _credit = CreditOracleClient::new(&env, &credit_id);
        let revocation = RevocationRegistryClient::new(&env, &revocation_id);

        let admin = soroban_sdk::Address::generate(&env);
        revocation.initialize(&admin);

        // Two different issuers
        let issuer_a = soroban_sdk::Address::generate(&env);
        let issuer_b = soroban_sdk::Address::generate(&env);

        // A VC hash that issuer_b should not be able to revoke after issuer_a registered it
        let vc_hash = BytesN::from_array(&env, &[7u8; 32]);

        // First revoke by issuer_a registers the authority.
        revocation.revoke(&issuer_a, &vc_hash);
        assert!(revocation.is_revoked(&vc_hash));

        // Second revoke by issuer_b must fail.
        let res = revocation.try_revoke(&issuer_b, &vc_hash);
        assert_eq!(
            res,
            Err(Ok(
                revocation_registry::RevocationRegistryError::IssuerMismatch
            ))
        );
    }

    #[test]
    fn test_batch_revoke_integration() {
        let env = Env::default();
        env.mock_all_auths();

        // 1. Register and initialize all 3 contracts
        let identity_id = env.register_contract(None, IdentityOracle);
        let credit_id = env.register_contract(None, CreditOracle);
        let revocation_id = env.register_contract(None, RevocationRegistry);

        let identity = IdentityOracleClient::new(&env, &identity_id);
        let credit = CreditOracleClient::new(&env, &credit_id);
        let revocation = RevocationRegistryClient::new(&env, &revocation_id);

        let admin = soroban_sdk::Address::generate(&env);
        identity.initialize(&admin);
        credit.initialize(&admin);
        revocation.initialize(&admin);

        // 2. Register issuer
        let issuer = soroban_sdk::Address::generate(&env);
        identity.register_issuer(&issuer);

        // 3. Create subject and DID
        let subject = soroban_sdk::Address::generate(&env);
        let cid = String::from_str(&env, "ipfs://QmBatchTestDID");
        identity.anchor_did(&subject, &cid);

        // 4. Anchor 5 VCs for the subject
        let mut vc_hashes = soroban_sdk::Vec::new(&env);
        for i in 0..5u8 {
            let mut hash_arr = [0u8; 32];
            hash_arr[0] = i;
            let vc_hash = BytesN::from_array(&env, &hash_arr);
            identity.anchor_vc(&issuer, &subject, &vc_hash);
            vc_hashes.push_back(vc_hash);
        }

        // 5. Assert is_verified is true (5 active VCs)
