use lazy_static;

use openmls::prelude::*;
use openmls::{
    credentials::{CredentialBundle, CredentialType},
    prelude::SignatureScheme,
};

lazy_static! {
static ref MLS_GROUP_CONFIG: MlsGroupConfig = MlsGroupConfig::builder()
    .padding_size(100)
    .sender_ratchet_configuration(SenderRatchetConfiguration::new(
        10,   // out_of_order_tolerance
        2000, // maximum_forward_distance
    ))
    .use_ratchet_tree_extension(true)
    .build();
}

pub fn generate_credential_bundle_from_identity(
    identity: Vec<u8>,
    backend: &impl OpenMlsCryptoProvider,
) -> Result<Credential, CredentialError> {
    generate_credential_bundle(
        identity,
        CredentialType::Basic,
        SignatureScheme::ED25519,
        backend,
    )
}

// A helper to create and store credentials.
fn generate_credential_bundle(
    identity: Vec<u8>,
    credential_type: CredentialType,
    signature_algorithm: SignatureScheme,
    backend: &impl OpenMlsCryptoProvider,
) -> Result<Credential, CredentialError> {
    let credential_bundle =
        CredentialBundle::new(identity, credential_type, signature_algorithm, backend)?;
    let credential_id = credential_bundle
        .credential()
        .signature_key()
        .tls_serialize_detached()
        .expect("Error serializing signature key.");
    // Store the credential bundle into the key store so OpenMLS has access
    // to it.
    backend
        .key_store()
        .store(&credential_id, &credential_bundle)
        .expect("An unexpected error occurred.");
    Ok(credential_bundle.into_parts().0)
}
pub fn generate_mls_group_from_welcome(
    backend: &impl OpenMlsCryptoProvider,
    welcome: Welcome,
) -> Result<MlsGroup, WelcomeError> {
    MlsGroup::new_from_welcome(
        backend,
        &MLS_GROUP_CONFIG,
        welcome,
        None, // We use the ratchet tree extension, so we don't provide a ratchet tree here
    )
}

pub fn generate_mls_group(
    backend: &impl OpenMlsCryptoProvider,
    key_package: KeyPackage,
) -> MlsGroup {
    let group_id = GroupId::from_slice(b"Test Group");
    MlsGroup::new(
        backend,
        &MLS_GROUP_CONFIG,
        group_id,
        key_package
            .hash_ref(backend.crypto())
            .expect("Could not hash KeyPackage.")
            .as_slice(),
    )
    .expect("An unexpected error occurred.")
}

// A helper to create key package bundles.
pub fn generate_key_package_bundle(
    credential: &Credential,
    backend: &impl OpenMlsCryptoProvider,
) -> Result<KeyPackage, KeyPackageBundleNewError> {
    // Fetch the credential bundle from the key store
    let credential_id = credential
        .signature_key()
        .tls_serialize_detached()
        .expect("Error serializing signature key.");
    let credential_bundle = backend
        .key_store()
        .read(&credential_id)
        .expect("An unexpected error occurred.");

    // Create the key package bundle
    let key_package_bundle = KeyPackageBundle::new(
        &[Ciphersuite::MLS_128_DHKEMX25519_CHACHA20POLY1305_SHA256_Ed25519],
        &credential_bundle,
        backend,
        vec![],
    )?;

    // Store it in the key store
    let key_package_id = key_package_bundle
        .key_package()
        .hash_ref(backend.crypto())
        .expect("Could not hash KeyPackage.");
    backend
        .key_store()
        .store(key_package_id.value(), &key_package_bundle)
        .expect("An unexpected error occurred.");
    Ok(key_package_bundle.into_parts().0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmls_rust_crypto::OpenMlsRustCrypto;

    #[test]
    fn smoke_test() -> Result<(), ()> {
        let backend = &OpenMlsRustCrypto::default();

        let bob_credential =
            generate_credential_bundle_from_identity("Bob1".into(), backend).unwrap();
        let alice_credential =
            generate_credential_bundle_from_identity("Alice1".into(), backend).unwrap();

        let bob_key_package = generate_key_package_bundle(&bob_credential, backend).unwrap();
        let alice_key_package = generate_key_package_bundle(&alice_credential, backend).unwrap();

        let group_id = GroupId::from_slice(b"Test Group");

        // Here is the group
        let mut alice_group = MlsGroup::new(
            backend,
            &MLS_GROUP_CONFIG,
            group_id,
            alice_key_package
                .hash_ref(backend.crypto())
                .expect("Could not hash KeyPackage.")
                .as_slice(),
        )
        .expect("An unexpected error occurred.");

        let (_, welcome) = alice_group
            .add_members(backend, &[bob_key_package])
            .expect("Could not add members.");

        // Join a group from a welcome message
        alice_group
            .merge_pending_commit()
            .expect("error merging pending commit");
        // Now Maxim can join the group.

        let mut bob_group = MlsGroup::new_from_welcome(
            backend,
            &MLS_GROUP_CONFIG,
            welcome,
            // The public tree is need and transferred out of band.
            // It is also possible to use the [`RatchetTreeExtension`]
            //Some(alice_group.export_ratchet_tree()),
            None,
        )
        .expect("Error joining group from Welcome");

        // try sending some messages and then updating commit package
        let message_alice = b"Hi, I'm Alice!";
        let mls_message_out = alice_group
            .create_message(backend, message_alice)
            .expect("Error creating application message.");

        let unverified_message = bob_group
            .parse_message(mls_message_out.into(), backend)
            .expect("Could not parse message.");

        let processed_message = bob_group
            .process_unverified_message(
                unverified_message,
                None, // No external signature key
                backend,
            )
            .expect("Could not process unverified message.");

        if let ProcessedMessage::ApplicationMessage(application_message) = processed_message {
            // Check the message
            assert_eq!(application_message.into_bytes(), b"Hi, I'm Alice!");
        }
        Ok(())
    }
}
