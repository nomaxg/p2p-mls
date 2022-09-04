use libp2p::{identity::Keypair, PeerId};
use openmls::{
    group::MlsGroup,
    prelude::{KeyPackage, MlsMessageOut, ProcessedMessage, Welcome},
};
use openmls_rust_crypto::OpenMlsRustCrypto;

use crate::{
    crypto::{
        generate_credential_bundle_from_identity, generate_key_package_bundle, generate_mls_group,
        generate_mls_group_from_welcome,
    },
    error::NodeError,
};

#[derive(Debug)]
struct Identity {
    network_key: Keypair,
    key_package: KeyPackage,
}

#[derive(Debug)]
pub struct Node {
    backend: OpenMlsRustCrypto,
    mls_group: Option<MlsGroup>,
    identity: Identity,
    is_group_leader: bool, // Only group leader can add new members to the group
}

impl Default for Node {
    fn default() -> Node {
        let backend = OpenMlsRustCrypto::default();
        let network_key = Keypair::generate_ed25519();
        let peer_id = PeerId::from_public_key(&network_key.public());
        let credential = generate_credential_bundle_from_identity(peer_id.into(), &backend)
            .expect("error creating credential");
        let key_package = generate_key_package_bundle(&credential, &backend)
            .expect("should have no problem with key package");

        Node {
            backend,
            mls_group: None,
            is_group_leader: false,
            identity: Identity {
                network_key,
                key_package,
            },
        }
    }
}

impl Node {
    pub fn join_new_group(&mut self) {
        self.mls_group = Some(generate_mls_group(
            &self.backend,
            self.identity.key_package.clone(),
        ));
        self.is_group_leader = true;
    }

    pub fn is_group_leader(&self) -> bool {
        self.is_group_leader
    }

    pub fn add_member_to_group(&mut self, key_package: KeyPackage) -> (MlsMessageOut, Welcome) {
        let group = self.mls_group.as_mut().expect("group expected");
        let (m_out, welcome) = group
            .add_members(&self.backend, &[key_package])
            .expect("Could not add members.");
        group
            .merge_pending_commit()
            .expect("error merging pending commit");
        (m_out, welcome)
    }

    pub fn join_existing_group(&mut self, welcome: Welcome) -> Result<(), NodeError> {
        self.mls_group = Some(generate_mls_group_from_welcome(&self.backend, welcome)?);
        self.is_group_leader = false;
        Ok(())
    }

    pub fn create_message(&mut self, msg: &str) -> Result<MlsMessageOut, NodeError> {
        Ok(self
            .mls_group
            .as_mut()
            .ok_or_else(|| NodeError("Group required to create message".to_string()))?
            .create_message(&self.backend, msg.as_bytes())
            .expect("Error creating application message."))
    }

    pub fn get_key_package(&self) -> KeyPackage {
        self.identity.key_package.clone()
    }

    pub fn get_network_keypair(&self) -> Keypair {
        self.identity.network_key.clone()
    }

    pub fn parse_message(&mut self, msg_out: MlsMessageOut) -> Result<Option<String>, NodeError> {
        if self.mls_group.is_none() {
            return Ok(None);
        }
        let unverified_message = self
            .mls_group
            .as_mut()
            .expect("group")
            .parse_message(msg_out.into(), &self.backend)?;

        let processed_message = self
            .mls_group
            .as_mut()
            .expect("group")
            .process_unverified_message(
                unverified_message,
                None, // No external signature key
                &self.backend,
            )
            .expect("Could not process unverified message.");

        if let ProcessedMessage::ApplicationMessage(application_message) = processed_message {
            // Check the message
            return Ok(Some(
                String::from_utf8(application_message.into_bytes()).unwrap(),
            ));
        } else if let ProcessedMessage::StagedCommitMessage(staged_commit) = processed_message {
            self.mls_group
                .as_mut()
                .expect("group")
                .merge_staged_commit(*staged_commit)
                .expect("Could not merge Commit.");
        }
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use openmls::prelude::TlsSerializeTrait;

    #[test]
    fn smoke_test() {
        let mut alice = Node::default();
        alice.join_new_group();
        let mut bob = Node::default();
        let bob_key_package = bob.get_key_package();
        let serialized = bob_key_package.tls_serialize_detached().unwrap();
        let bytes_array: &[u8] = &serialized;
        let (_, welcome) = alice.add_member_to_group(KeyPackage::try_from(bytes_array).unwrap());
        //bob.join_new_group(); TODO figure out why this causes an error
        bob.join_existing_group(welcome).expect("");
        let msg_out = alice.create_message("hi bob").unwrap();
        let msg = bob
            .parse_message(msg_out.unwrap())
            .expect("message parsed")
            .unwrap();
        assert_eq!(msg, "hi bob");
    }
}
