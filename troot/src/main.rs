use bitcoin::{
    address::Address, blockdata::{opcodes, script::Builder, transaction::{OutPoint, Transaction, TxIn, TxOut}}, consensus::encode::serialize, hashes::hex::BytesToHexIter, hex::{Case, DisplayHex}, key::{Keypair, TapTweak, TweakedKeypair}, network::Network, script::ScriptBuf, secp256k1::{rand, Secp256k1, Signing}, sighash::{Prevouts, SighashCache, TapSighashType}, taproot::{LeafVersion, Signature, TapLeafHash, TaprootBuilder, TaprootSpendInfo}, Amount, Sequence, Txid
};
use std::str::FromStr;
use ureq;


fn broadcast_tx(tx: &Transaction) {
    let raw_tx = serialize(tx).to_lower_hex_string();
    let url = "https://blockstream.info/testnet/api/tx";

    let response = ureq::post(url)
        .content_type("text/plain")
        .send(&raw_tx);

    match response {
        Ok(r) => {
            println!("✅ Broadcast successful! Txid: {}", tx.compute_txid());
            println!("Server says: {:?}", r.body());
        },
        Err(e) => {
            eprintln!("❌ Broadcast failed: {}", e);
        }
    }
}

fn main() {
    let secp = Secp256k1::new();

    // 1. Generate 3 keypairs
    let sk1 = Keypair::new(&secp, &mut rand::thread_rng());
    let sk2 = Keypair::new(&secp, &mut rand::thread_rng());
    let sk3 = Keypair::new(&secp, &mut rand::thread_rng());

    let pk1 = sk1.public_key();
    let pk2 = sk2.public_key();
    let pk3 = sk3.public_key();

    // 2. Build the script: 2-of-3 using CHECKSIGADD
    let tapscript = Builder::new()
        .push_key(&pk1)
        .push_opcode(opcodes::all::OP_CHECKSIG)
        .push_key(&pk2)
        .push_opcode(opcodes::all::OP_CHECKSIGADD)
        .push_key(&pk3)
        .push_opcode(opcodes::all::OP_CHECKSIGADD)
        .push_int(2)
        .push_opcode(opcodes::all::OP_NUMEQUAL)
        .into_script();

    // 3. Taproot output construction (script-path only)
    let mut builder = TaprootBuilder::new();
    builder = builder.add_leaf(0, tapscript.clone()).unwrap();

    let internal_key = bitcoin::XOnlyPublicKey::from(pk1);
    let spend_info = builder.finalize(&secp, internal_key).unwrap();

    let taproot_address = Address::p2tr_tweaked(spend_info.output_key(), Network::Testnet);
    println!("Taproot script-path address (fund this!): {}", taproot_address);

    // 4. FUND the above address with a testnet faucet. Set txid & vout accordingly.
    let utxo_txid = Txid::from_str("REPLACE_WITH_FUNDED_TXID").unwrap();
    let utxo_vout = 0; // Or whatever output index
    let utxo_value_sat = 10000;

    // 5. Construct the spending transaction (send to some testnet address you control)
    let destination = Address::from_str("REPLACE_WITH_YOUR_TESTNET_ADDRESS").unwrap();

    let input = TxIn {
        previous_output: OutPoint::new(utxo_txid, utxo_vout),
        script_sig: ScriptBuf::new(),
        sequence: Sequence::ENABLE_RBF_NO_LOCKTIME,
        witness: bitcoin::Witness::new(),
    };

    let output = TxOut {
        value: utxo_value_sat - 500, // Subtract fee
        script_pubkey: destination.script_pubkey(),
    };

    let mut tx = Transaction {
        version: 2,
        lock_time: bitcoin::LockTime::ZERO,
        input: vec![input],
        output: vec![output],
    };

    // 6. Generate Schnorr signatures (for sk1 and sk2 here)
    let mut sighash_cache = SighashCache::new(&mut tx);
    let prevouts = Prevouts::All(&[TxOut {
        value: utxo_value_sat,
        script_pubkey: spend_info.script_pubkey(),
    }]);

    let sighash = sighash_cache
        .taproot_script_spend_signature_hash(
            0,
            &prevouts,
            TapLeafHash::from_script(&tapscript, LeafVersion::TapScript),
            TapSighashType::Default,
        )
        .unwrap();

    let sig1 = secp.sign_schnorr(&sighash, &sk1);
    let sig2 = secp.sign_schnorr(&sighash, &sk2);

    // 7. Build the witness stack (signatures in order of pubkeys used in script)
    let control_block = spend_info
        .control_block(&(tapscript.clone(), LeafVersion::TapScript))
        .unwrap();

    tx.input[0].witness.push(sig1.as_ref()); // sig1
    tx.input[0].witness.push(sig2.as_ref()); // sig2
    tx.input[0].witness.push(tapscript.clone().into_bytes()); // tapscript
    tx.input[0].witness.push(control_block.serialize()); // control block

    println!("Raw tx: {}", serialize(&tx).to_hex());
    broadcast_tx(&tx);
    // ➕ Broadcast using any testnet explorer (like mempool.space or blockstream)
}
