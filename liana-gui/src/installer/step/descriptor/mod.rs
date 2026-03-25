pub mod editor;

use std::{
    collections::{HashMap, HashSet},
    str::FromStr,
};

use iced::{Subscription, Task};
use liana::{
    descriptors::LianaDescriptor,
    miniscript::bitcoin::{bip32::Fingerprint, Network},
};

use liana_ui::{component::form, widget::Element};

use async_hwi::DeviceKind;

use crate::{
    app::{
        settings::{ContactSetting, KeySetting},
        state::export::ExportModal,
        wallet::wallet_name,
    },
    backup::Backup,
    export::{ImportExportMessage, ImportExportType, Progress},
    hw::{HardwareWallet, HardwareWallets},
    installer::{
        decrypt::{Decrypt, DecryptModal},
        message::{self, Message},
        step::import_descriptor::{ImportDescriptorModal, BACKUP_NETWORK_NOT_MATCH},
        step::{Context, Step},
        view, Error,
    },
};

pub struct ImportDescriptor {
    network: Network,
    wrong_network: bool,
    error: Option<String>,
    modal: ImportDescriptorModal,
    imported_descriptor: form::Value<String>,
    imported_backup: Option<Backup>,
    imported_aliases: Option<HashMap<Fingerprint, KeySetting>>,
}

impl ImportDescriptor {
    pub fn new(network: Network) -> Self {
        Self {
            network,
            imported_descriptor: form::Value::default(),
            wrong_network: false,
            error: None,
            modal: ImportDescriptorModal::None,
            imported_backup: None,
            imported_aliases: None,
        }
    }

    fn check_descriptor(&mut self, network: Network) -> Option<LianaDescriptor> {
        if !self.imported_descriptor.value.is_empty() {
            if let Ok(desc) = LianaDescriptor::from_str(&self.imported_descriptor.value) {
                if network == Network::Bitcoin {
                    self.imported_descriptor.valid = desc.all_xpubs_net_is(network);
                } else {
                    self.imported_descriptor.valid = desc.all_xpubs_net_is(Network::Testnet);
                }
                if self.imported_descriptor.valid {
                    self.wrong_network = false;
                    Some(desc)
                } else {
                    self.wrong_network = true;
                    None
                }
            } else {
                self.imported_descriptor.valid = false;
                self.wrong_network = false;
                None
            }
        } else {
            self.wrong_network = false;
            self.imported_descriptor.valid = true;
            None
        }
    }
}

impl Step for ImportDescriptor {
    // ImportRemoteWallet is used instead
    fn skip(&self, ctx: &Context) -> bool {
        ctx.remote_backend.is_some()
    }

    fn subscription(&self, hws: &HardwareWallets) -> Subscription<Message> {
        self.modal.subscriptions(hws)
    }

    fn update(&mut self, hws: &mut HardwareWallets, message: Message) -> Task<Message> {
        let task = match message {
            Message::DefineDescriptor(message::DefineDescriptor::ImportDescriptor(desc)) => {
                // If user manually change the descriptor, then the imported backup
                // becomes invalid;
                if desc != self.imported_descriptor.value {
                    self.imported_backup = None;
                    self.imported_aliases = None;
                }
                self.imported_descriptor.value = desc;
                self.check_descriptor(self.network);
                None
            }
            Message::ImportBackup => {
                self.imported_backup = None;
                let modal = ExportModal::new(None, ImportExportType::FromBackup);
                let launch = modal.launch(false);
                self.modal = ImportDescriptorModal::Export(modal);
                Some(launch)
            }
            Message::ImportExport(ImportExportMessage::Close) => {
                self.modal = ImportDescriptorModal::None;
                None
            }
            Message::ImportExport(ImportExportMessage::Progress(Progress::WalletFromBackup(r))) => {
                let (descriptor, network, aliases, backup) = r;
                if let Some(n) = network {
                    if self.network == n {
                        self.imported_backup = Some(backup);
                        self.imported_descriptor.value = descriptor.to_string();
                        self.imported_aliases = Some(aliases);
                    } else {
                        self.error = Some(BACKUP_NETWORK_NOT_MATCH.into());
                    }
                } else {
                    // The backup have been inferred from a bare descriptor, we check whether
                    // the descriptor match any test network
                    if self.network != Network::Bitcoin {
                        self.imported_backup = Some(backup);
                        self.imported_descriptor.value = descriptor.to_string();
                        self.imported_aliases = Some(aliases);
                    } else {
                        self.error = Some(BACKUP_NETWORK_NOT_MATCH.into());
                    }
                }
                None
            }
            Message::ImportExport(ImportExportMessage::Progress(Progress::EncryptedFile(
                bytes,
            ))) => {
                self.modal = ImportDescriptorModal::Decrypt(DecryptModal::new(bytes, self.network));
                None
            }
            Message::ImportExport(m) => Some(self.modal.update(Message::ImportExport(m))),
            Message::HardwareWalletUpdate => {
                if let ImportDescriptorModal::Decrypt(modal) = &mut self.modal {
                    modal.update_devices(hws)
                } else {
                    None
                }
            }
            Message::Decrypt(Decrypt::Close) => {
                if matches!(self.modal, ImportDescriptorModal::Decrypt(_)) {
                    self.modal = ImportDescriptorModal::None;
                }
                None
            }
            Message::Decrypt(Decrypt::Backup(mut backup)) => {
                let descriptor = backup.accounts.first().map(|acc| acc.descriptor.clone());
                if let Some(desc) = descriptor {
                    let network_matches = if self.network == Network::Bitcoin {
                        backup.network == Network::Bitcoin
                    } else {
                        backup.network != Network::Bitcoin
                    };
                    if network_matches {
                        // NOTE: we need to overwrite w/ correct network for testnets
                        // as non Mainnet keys / descriptor are parsed as Signet
                        backup.network = self.network;

                        self.imported_descriptor.value = desc;
                        self.imported_backup = Some(backup);
                        self.imported_aliases = None;
                        self.modal = ImportDescriptorModal::None;
                    } else {
                        self.modal = ImportDescriptorModal::None;
                        self.error = Some(BACKUP_NETWORK_NOT_MATCH.into());
                    }
                } else {
                    self.modal = ImportDescriptorModal::None;
                    self.error = Some("Backup imported but descriptor missing!".into());
                }
                None
            }
            Message::Decrypt(msg) => Some(self.modal.update(Message::Decrypt(msg))),
            _ => None,
        };
        task.unwrap_or(Task::none())
    }

    fn apply(&mut self, ctx: &mut Context) -> bool {
        ctx.bitcoin_config.network = self.network;
        // Set to true in order to force the registration process to be shown to user.
        ctx.hw_is_used = true;
        // descriptor forms for import or creation cannot be both empty or filled.
        if let Some(desc) = self.check_descriptor(self.network) {
            ctx.descriptor = Some(desc);
        } else {
            return false;
        }

        if let Some(backup) = &self.imported_backup {
            ctx.backup = Some(backup.clone());
        }

        if let Some(aliases) = &self.imported_aliases {
            ctx.keys = aliases.clone();
        }

        if let Some(wallet_alias) = self.imported_backup.as_ref().and_then(|b| b.alias.clone()) {
            ctx.wallet_alias = wallet_alias;
        }
        true
    }

    fn revert(&self, ctx: &mut Context) {
        ctx.keys = HashMap::new();
        ctx.backup = None;
        ctx.descriptor = None;
        ctx.wallet_alias = String::new();
    }

    fn view<'a>(
        &'a self,
        _hws: &'a HardwareWallets,
        progress: (usize, usize),
        email: Option<&'a str>,
    ) -> Element<'a, Message> {
        let content = view::import_descriptor(
            progress,
            email,
            &self.imported_descriptor,
            self.imported_backup.is_some(),
            self.wrong_network,
            self.error.as_ref(),
        );
        self.modal.view(content)
    }
}

impl From<ImportDescriptor> for Box<dyn Step> {
    fn from(s: ImportDescriptor) -> Box<dyn Step> {
        Box::new(s)
    }
}

pub struct RegisterDescriptor {
    descriptor: Option<LianaDescriptor>,
    processing: bool,
    chosen_hw: Option<usize>,
    hmacs: Vec<(Fingerprint, DeviceKind, Option<[u8; 32]>)>,
    registered: HashSet<Fingerprint>,
    error: Option<Error>,
    done: bool,
    /// Whether this step is part of the descriptor creation process. This is used to detect when
    /// it's instead shown as part of the descriptor *import* process, where we can't detect
    /// whether a signing device is used, to explicit this step is not required if the user isn't
    /// using a signing device.
    created_desc: bool,
    /// Identity signatures for descriptor keys: fingerprint → (id_pubkey_hex, id_sig_hex).
    key_identity_sigs: HashMap<Fingerprint, (String, String)>,
    /// Contacts with registered identity keys for the current wallet.
    contacts: Vec<ContactSetting>,
}

impl RegisterDescriptor {
    fn new(created_desc: bool) -> Self {
        Self {
            created_desc,
            descriptor: Default::default(),
            processing: Default::default(),
            chosen_hw: Default::default(),
            hmacs: Default::default(),
            registered: Default::default(),
            error: Default::default(),
            done: Default::default(),
            key_identity_sigs: Default::default(),
            contacts: Default::default(),
        }
    }

    pub fn new_create_wallet() -> Self {
        Self::new(true)
    }

    pub fn new_import_wallet() -> Self {
        Self::new(false)
    }
}

impl Step for RegisterDescriptor {
    fn load_context(&mut self, ctx: &Context) {
        // we reset device registered set if the descriptor have changed.
        if self.descriptor != ctx.descriptor {
            self.registered = Default::default();
            self.done = false;
        }
        self.descriptor.clone_from(&ctx.descriptor);
        self.key_identity_sigs = ctx.key_identity_sigs.clone();
        self.contacts = ctx.contacts.clone();
        let mut map = HashMap::new();
        for key in ctx.keys.values().filter(|k| !k.name.is_empty()) {
            map.insert(key.master_fingerprint, key.name.clone());
        }
    }
    fn update(&mut self, hws: &mut HardwareWallets, message: Message) -> Task<Message> {
        match message {
            Message::Select(i) => {
                if let Some(HardwareWallet::Supported {
                    device,
                    fingerprint,
                    ..
                }) = hws.list.get(i)
                {
                    if !self.registered.contains(fingerprint) {
                        let descriptor = self.descriptor.as_ref().unwrap();
                        let name = wallet_name(descriptor);
                        self.chosen_hw = Some(i);
                        self.processing = true;
                        self.error = None;
                        return Task::perform(
                            register_wallet(
                                device.clone(),
                                *fingerprint,
                                name,
                                descriptor.to_string(),
                                self.key_identity_sigs.clone(),
                                self.contacts.clone(),
                                descriptor.clone(),
                            ),
                            Message::WalletRegistered,
                        );
                    }
                }
            }
            Message::WalletRegistered(res) => {
                self.processing = false;
                self.chosen_hw = None;
                match res {
                    Ok((fingerprint, hmac)) => {
                        if let Some(hw_h) = hws
                            .list
                            .iter()
                            .find(|hw_h| hw_h.fingerprint() == Some(fingerprint))
                        {
                            self.registered.insert(fingerprint);
                            self.hmacs.push((fingerprint, *hw_h.kind(), hmac));
                        }
                    }
                    Err(e) => {
                        if !matches!(e, Error::HardwareWallet(async_hwi::Error::UserRefused)) {
                            self.error = Some(e)
                        }
                    }
                }
            }
            Message::Reload => {
                return self.load();
            }
            Message::UserActionDone(done) => {
                self.done = done;
            }
            _ => {}
        };
        Task::none()
    }
    fn skip(&self, ctx: &Context) -> bool {
        !ctx.hw_is_used
    }
    fn apply(&mut self, ctx: &mut Context) -> bool {
        for (fingerprint, kind, token) in &self.hmacs {
            ctx.hws.push((*kind, *fingerprint, *token));
        }
        true
    }
    fn subscription(&self, hws: &HardwareWallets) -> Subscription<Message> {
        hws.refresh().map(Message::HardwareWallets)
    }
    fn load(&self) -> Task<Message> {
        Task::none()
    }
    fn view<'a>(
        &'a self,
        hws: &'a HardwareWallets,
        progress: (usize, usize),
        email: Option<&'a str>,
    ) -> Element<'a, Message> {
        let desc = self.descriptor.as_ref().unwrap();

        view::register_descriptor(
            progress,
            email,
            desc,
            &hws.list,
            &self.registered,
            self.error.as_ref(),
            self.processing,
            self.chosen_hw,
            self.done,
            self.created_desc,
        )
    }
}

/// Build the `registered_identities` and `key_signatures` parameters for
/// `register_wallet_with_identities`.
///
/// Iterates over the descriptor keys (in descriptor order), and for each key that has an
/// identity signature whose `id_pubkey` matches a saved contact, builds the corresponding
/// `RegisteredIdentityEntry` and `IdentitySignature`.
///
/// Returns `(None, None)` if no key has a matching identity signature.
fn build_identity_params(
    descriptor: &LianaDescriptor,
    key_identity_sigs: &HashMap<Fingerprint, (String, String)>,
    contacts: &[ContactSetting],
) -> (
    Option<Vec<vnd_bitcoin_common::message::RegisteredIdentityEntry>>,
    Option<Vec<Option<vnd_bitcoin_common::message::IdentitySignature>>>,
) {
    use liana::miniscript::descriptor::DescriptorPublicKey;
    use liana::miniscript::{Descriptor, ForEachKey};

    // Collect unique keys in wallet policy order (matching the device's key
    // indexing). For Taproot descriptors, `for_each_key` visits script-tree
    // keys *before* the internal key (see miniscript's `Tr::for_each_key`),
    // but the wallet policy indexes the internal key as @0 (first in the
    // descriptor string). We therefore handle Taproot explicitly.
    // TODO: this is flaky, we should have a better way of doing it. Ideally, Liana might store the
    // descriptor template and key info vector _instead_ of the descriptor, which would make the
    // ordering of keys explicit and avoid the conversion.
    let mut seen = HashSet::new();
    let mut ordered_fingerprints = Vec::new();

    let push_fingerprint =
        |k: &DescriptorPublicKey, seen: &mut HashSet<Fingerprint>, out: &mut Vec<Fingerprint>| {
            if let DescriptorPublicKey::MultiXPub(mxk) = k {
                if let Some((fg, _)) = &mxk.origin {
                    if seen.insert(*fg) {
                        out.push(*fg);
                    }
                }
            }
        };

    match descriptor.descriptor() {
        Descriptor::Tr(tr) => {
            // Internal key first (wallet policy @0).
            push_fingerprint(tr.internal_key(), &mut seen, &mut ordered_fingerprints);
            // Then script-tree keys in tree iteration order.
            for (_depth, ms) in tr.iter_scripts() {
                ms.for_each_key(|k| {
                    push_fingerprint(k, &mut seen, &mut ordered_fingerprints);
                    true
                });
            }
        }
        other => {
            other.for_each_key(|k| {
                push_fingerprint(k, &mut seen, &mut ordered_fingerprints);
                true
            });
        }
    }

    let mut identities = Vec::new();
    let mut signatures: Vec<Option<vnd_bitcoin_common::message::IdentitySignature>> = Vec::new();
    let mut has_any = false;

    for fg in &ordered_fingerprints {
        if let Some((id_pubkey_hex, id_sig_hex)) = key_identity_sigs.get(fg) {
            // Check if id_pubkey matches a saved contact with registrations
            let contact = contacts.iter().find(|c| c.pubkey == *id_pubkey_hex);
            if let Some(contact) = contact {
                if let Some(reg) = contact.registrations.first() {
                    if let (Ok(pk_bytes), Ok(sig_bytes), Ok(por_bytes)) = (
                        hex::decode(id_pubkey_hex),
                        hex::decode(id_sig_hex),
                        hex::decode(&reg.proof),
                    ) {
                        // Add to registered identities (deduplicated)
                        if !identities.iter().any(
                            |ri: &vnd_bitcoin_common::message::RegisteredIdentityEntry| {
                                ri.pubkey == pk_bytes
                            },
                        ) {
                            identities.push(vnd_bitcoin_common::message::RegisteredIdentityEntry {
                                pubkey: pk_bytes.clone(),
                                name: contact.name.clone(),
                                por: por_bytes,
                            });
                        }
                        signatures.push(Some(vnd_bitcoin_common::message::IdentitySignature {
                            identity_pubkey: pk_bytes,
                            signature: sig_bytes,
                        }));
                        has_any = true;
                        continue;
                    }
                }
            }
        }
        signatures.push(None);
    }

    if has_any {
        (Some(identities), Some(signatures))
    } else {
        (None, None)
    }
}

async fn register_wallet(
    hw: std::sync::Arc<dyn async_hwi::HWI + Send + Sync>,
    fingerprint: Fingerprint,
    name: String,
    descriptor: String,
    key_identity_sigs: HashMap<Fingerprint, (String, String)>,
    contacts: Vec<ContactSetting>,
    liana_descriptor: LianaDescriptor,
) -> Result<(Fingerprint, Option<[u8; 32]>), Error> {
    // Check if any descriptor keys have identity signatures matching a saved contact.
    let (registered_identities, key_signatures) =
        build_identity_params(&liana_descriptor, &key_identity_sigs, &contacts);

    if registered_identities.is_some() || key_signatures.is_some() {
        let (reg_id, _por) = hw
            .register_wallet_with_identities(
                &name,
                &descriptor,
                registered_identities,
                key_signatures,
            )
            .await
            .map_err(Error::from)?;
        // The registration ID is a 32-byte HMAC; convert it for storage.
        Ok((fingerprint, Some(*reg_id.as_bytes())))
    } else {
        let hmac = hw
            .register_wallet(&name, &descriptor)
            .await
            .map_err(Error::from)?;
        Ok((fingerprint, hmac))
    }
}

impl From<RegisterDescriptor> for Box<dyn Step> {
    fn from(s: RegisterDescriptor) -> Box<dyn Step> {
        Box::new(s)
    }
}

#[derive(Default)]
pub struct BackupDescriptor {
    done: bool,
    descriptor: Option<LianaDescriptor>,
    keys: HashMap<Fingerprint, KeySetting>,
    modal: Option<ExportModal>,
    error: Option<Error>,
    context: Option<Context>,
}

impl Step for BackupDescriptor {
    fn subscription(&self, _hws: &HardwareWallets) -> Subscription<Message> {
        if let Some(modal) = &self.modal {
            if let Some(sub) = modal.subscription() {
                sub.map(|m| Message::ImportExport(ImportExportMessage::Progress(m)))
            } else {
                Subscription::none()
            }
        } else {
            Subscription::none()
        }
    }
    fn update(&mut self, _hws: &mut HardwareWallets, message: Message) -> Task<Message> {
        match message {
            Message::ImportExport(ImportExportMessage::Close) => {
                self.modal = None;
            }
            Message::ImportExport(m) => {
                if let Some(modal) = self.modal.as_mut() {
                    let task: Task<Message> = modal.update(m);
                    return task;
                };
            }
            Message::BackupDescriptor => {
                if let (None, Some(ctx)) = (&self.modal, self.context.as_ref()) {
                    let descriptor = ctx.descriptor.clone();
                    return Task::perform(
                        async move {
                            let descriptor = descriptor.ok_or(encrypted_backup::Error::String(
                                Box::new("Descriptor missing".to_string()),
                            ))?;
                            Ok(Box::new(descriptor))
                        },
                        Message::ExportEncryptedDescriptor,
                    );
                }
            }
            Message::ExportEncryptedDescriptor(bytes) => {
                if self.modal.is_none() {
                    let bytes = match bytes {
                        Ok(b) => b,
                        Err(e) => {
                            tracing::error!("{e:?}");
                            self.error = Some(Error::Backup(e));
                            return Task::none();
                        }
                    };
                    let modal =
                        ExportModal::new(None, ImportExportType::ExportEncryptedDescriptor(bytes));
                    let launch = modal.launch(true);
                    self.modal = Some(modal);
                    return launch;
                }
            }
            Message::UserActionDone(done) => {
                self.done = done;
            }
            _ => {}
        }
        Task::none()
    }
    fn load_context(&mut self, ctx: &Context) {
        self.context = Some(ctx.clone());
        if self.descriptor != ctx.descriptor {
            self.descriptor.clone_from(&ctx.descriptor);
            self.done = false;
        }
        self.keys = ctx
            .keys
            .values()
            .map(|k| (k.master_fingerprint, k.clone()))
            .collect();
    }
    fn view<'a>(
        &'a self,
        _hws: &'a HardwareWallets,
        progress: (usize, usize),
        email: Option<&'a str>,
    ) -> Element<'a, Message> {
        let content = view::backup_descriptor(
            progress,
            email,
            self.descriptor.as_ref().expect("Must be a descriptor"),
            &self.keys,
            self.error.as_ref(),
            self.done,
        );
        if let Some(modal) = &self.modal {
            modal.view(content)
        } else {
            content
        }
    }
}

impl From<BackupDescriptor> for Box<dyn Step> {
    fn from(s: BackupDescriptor) -> Box<dyn Step> {
        Box::new(s)
    }
}
