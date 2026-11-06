//! Who actually authorized a Soroban contract call.
//!
//! With relayers, fee sponsors and smart wallets, the account that submits a
//! transaction (the operation's source) is often a service, not the user.
//! The user appears in the transaction's authorization entries instead: each
//! `Address` credential names an account (G...) or smart-wallet contract
//! (C...) that signed for the call. Fee bumps are unwrapped to the inner
//! transaction. Calls authorized only by the source account have no such
//! entries, and the source account is the user.

use stellar_xdr::{
    FeeBumpTransactionInnerTx, Limits, OperationBody, ReadXdr, SorobanCredentials, TransactionEnvelope,
};

/// Accounts or contracts that signed authorization entries for the contract
/// call in a transaction (base64 envelope XDR), in order, without repeats.
/// Empty when the call relies on source-account authorization or the
/// envelope can't be read.
pub fn authorizers(envelope_b64: &str) -> Vec<String> {
    let Ok(envelope) = TransactionEnvelope::from_xdr_base64(envelope_b64, Limits::none()) else {
        return vec![];
    };
    let operations = match &envelope {
        TransactionEnvelope::Tx(e) => &e.tx.operations,
        TransactionEnvelope::TxV0(e) => &e.tx.operations,
        TransactionEnvelope::TxFeeBump(f) => match &f.tx.inner_tx {
            FeeBumpTransactionInnerTx::Tx(e) => &e.tx.operations,
        },
    };
    let mut out: Vec<String> = Vec::new();
    for op in operations.iter() {
        let OperationBody::InvokeHostFunction(call) = &op.body else { continue };
        for entry in call.auth.iter() {
            let address = match &entry.credentials {
                SorobanCredentials::Address(c) | SorobanCredentials::AddressV2(c) => &c.address,
                SorobanCredentials::AddressWithDelegates(d) => &d.address_credentials.address,
                SorobanCredentials::SourceAccount => continue,
            };
            let id = address.to_string();
            if !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use stellar_xdr::{
        AccountId, ContractId, Hash, HostFunction, InvokeContractArgs, InvokeHostFunctionOp, Memo, MuxedAccount,
        Operation, Preconditions, PublicKey, ScAddress, ScSymbol, ScVal, SequenceNumber, SorobanAddressCredentials,
        SorobanAuthorizationEntry, SorobanAuthorizedFunction, SorobanAuthorizedInvocation, Transaction,
        TransactionExt, TransactionV1Envelope, Uint256, WriteXdr,
    };

    fn invoke_with_auth(credentials: Vec<SorobanCredentials>) -> String {
        let contract = ScAddress::Contract(ContractId(Hash([9; 32])));
        let args = InvokeContractArgs {
            contract_address: contract,
            function_name: ScSymbol("swap".try_into().unwrap()),
            args: vec![].try_into().unwrap(),
        };
        let auth: Vec<SorobanAuthorizationEntry> = credentials
            .into_iter()
            .map(|credentials| SorobanAuthorizationEntry {
                credentials,
                root_invocation: SorobanAuthorizedInvocation {
                    function: SorobanAuthorizedFunction::ContractFn(args.clone()),
                    sub_invocations: vec![].try_into().unwrap(),
                },
            })
            .collect();
        let op = Operation {
            source_account: None,
            body: OperationBody::InvokeHostFunction(InvokeHostFunctionOp {
                host_function: HostFunction::InvokeContract(args),
                auth: auth.try_into().unwrap(),
            }),
        };
        let tx = Transaction {
            // The relayer submits and pays.
            source_account: MuxedAccount::Ed25519(Uint256([1; 32])),
            fee: 100,
            seq_num: SequenceNumber(1),
            cond: Preconditions::None,
            memo: Memo::None,
            operations: vec![op].try_into().unwrap(),
            ext: TransactionExt::V0,
        };
        TransactionEnvelope::Tx(TransactionV1Envelope { tx, signatures: vec![].try_into().unwrap() })
            .to_xdr_base64(Limits::none())
            .unwrap()
    }

    fn address_cred(address: ScAddress) -> SorobanCredentials {
        SorobanCredentials::Address(SorobanAddressCredentials {
            address,
            nonce: 1,
            signature_expiration_ledger: 100,
            signature: ScVal::Void,
        })
    }

    #[test]
    fn finds_the_user_behind_a_relayer() {
        let user = ScAddress::Account(AccountId(PublicKey::PublicKeyTypeEd25519(Uint256([2; 32]))));
        let wallet = ScAddress::Contract(ContractId(Hash([3; 32])));
        let found = authorizers(&invoke_with_auth(vec![address_cred(user.clone()), address_cred(wallet.clone()), address_cred(user.clone())]));
        assert_eq!(found, vec![user.to_string(), wallet.to_string()]);
        assert!(found[0].starts_with('G') && found[1].starts_with('C'));
    }

    #[test]
    fn source_account_auth_has_no_separate_user() {
        assert!(authorizers(&invoke_with_auth(vec![SorobanCredentials::SourceAccount])).is_empty());
        assert!(authorizers("not xdr").is_empty());
    }
}
