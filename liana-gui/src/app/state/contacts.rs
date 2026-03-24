use std::sync::Arc;

use iced::{Subscription, Task};
use liana::miniscript::bitcoin::{
    bip32::{ChainCode, ChildNumber, Fingerprint, Xpub},
    secp256k1, Network,
};
use liana_ui::{component::form, widget::*};

use crate::{
    app::{
        cache::Cache,
        error::Error,
        menu::Menu,
        message::Message,
        settings::{
            update_settings_file, ContactRegistration, ContactSetting, IdentityKeySetting,
            LianaSettings,
        },
        state::State,
        view,
        wallet::Wallet,
    },
    daemon::Daemon,
    dir::LianaDirectory,
    hw::{HardwareWallet, HardwareWallets},
};

pub struct ContactsPanel {
    data_dir: LianaDirectory,
    wallet: Arc<Wallet>,
    network: Network,
    contacts: Vec<ContactSetting>,
    identity_keys: Vec<IdentityKeySetting>,
    name_input: form::Value<String>,
    pubkey_input: form::Value<String>,
    hws: HardwareWallets,
    warning: Option<Error>,
    /// Index of the contact currently being registered, if any.
    registering_contact: Option<usize>,
    /// Whether we are currently fetching an identity key from a device.
    fetching_identity_key: bool,
}

impl ContactsPanel {
    pub fn new(data_dir: LianaDirectory, wallet: Arc<Wallet>, network: Network) -> Self {
        let contacts = load_contacts(&data_dir, network, &wallet);
        let identity_keys = load_identity_keys(&data_dir, network, &wallet);
        Self {
            hws: HardwareWallets::new(data_dir.clone(), network),
            data_dir,
            wallet,
            network,
            contacts,
            identity_keys,
            name_input: form::Value::default(),
            pubkey_input: form::Value::default(),
            warning: None,
            registering_contact: None,
            fetching_identity_key: false,
        }
    }
}

pub(crate) fn load_contacts(
    data_dir: &LianaDirectory,
    network: Network,
    wallet: &Wallet,
) -> Vec<ContactSetting> {
    let network_dir = data_dir.network_directory(network);
    let wallet_id = wallet.id();
    LianaSettings::from_file(&network_dir)
        .ok()
        .and_then(|s| {
            s.wallets
                .into_iter()
                .find(|w| w.wallet_id() == wallet_id)
                .map(|w| w.contacts)
        })
        .unwrap_or_default()
}

fn load_identity_keys(
    data_dir: &LianaDirectory,
    network: Network,
    wallet: &Wallet,
) -> Vec<IdentityKeySetting> {
    let network_dir = data_dir.network_directory(network);
    let wallet_id = wallet.id();
    LianaSettings::from_file(&network_dir)
        .ok()
        .and_then(|s| {
            s.wallets
                .into_iter()
                .find(|w| w.wallet_id() == wallet_id)
                .map(|w| w.identity_keys)
        })
        .unwrap_or_default()
}

fn parse_pubkey_to_xpub(hex_str: &str) -> Result<Xpub, String> {
    if hex_str.len() != 66 {
        return Err("Public key must be a 66-character hex string (compressed pubkey)".to_string());
    }
    let pubkey_bytes = hex::decode(hex_str).map_err(|e| format!("Invalid hex: {}", e))?;
    let pubkey = secp256k1::PublicKey::from_slice(&pubkey_bytes)
        .map_err(|e| format!("Invalid public key: {}", e))?;
    Ok(Xpub {
        network: liana::miniscript::bitcoin::NetworkKind::Main,
        depth: 0,
        parent_fingerprint: Fingerprint::default(),
        child_number: ChildNumber::from_normal_idx(0).expect("valid"),
        chain_code: ChainCode::from([0u8; 32]),
        public_key: pubkey,
    })
}

fn save_contacts(
    data_dir: LianaDirectory,
    network: Network,
    wallet_id: crate::app::settings::WalletId,
    contacts: Vec<ContactSetting>,
) -> Task<Message> {
    Task::perform(
        async move {
            let network_dir = data_dir.network_directory(network);
            update_settings_file(&network_dir, |mut s: LianaSettings| {
                if let Some(ws) = s.wallets.iter_mut().find(|w| w.wallet_id() == wallet_id) {
                    ws.contacts = contacts;
                }
                s
            })
            .await
        },
        |res| match res {
            Ok(()) => Message::View(view::Message::Reload),
            Err(e) => Message::View(view::Message::Contacts(
                view::ContactsMessage::RegistrationFailed(format!(
                    "Failed to save contacts: {}",
                    e
                )),
            )),
        },
    )
}

fn save_identity_keys(
    data_dir: LianaDirectory,
    network: Network,
    wallet_id: crate::app::settings::WalletId,
    identity_keys: Vec<IdentityKeySetting>,
) -> Task<Message> {
    Task::perform(
        async move {
            let network_dir = data_dir.network_directory(network);
            update_settings_file(&network_dir, |mut s: LianaSettings| {
                if let Some(ws) = s.wallets.iter_mut().find(|w| w.wallet_id() == wallet_id) {
                    ws.identity_keys = identity_keys;
                }
                s
            })
            .await
        },
        |res| match res {
            Ok(()) => Message::View(view::Message::Reload),
            Err(e) => Message::View(view::Message::Contacts(
                view::ContactsMessage::IdentityKeyFailed(format!(
                    "Failed to save identity key: {}",
                    e
                )),
            )),
        },
    )
}

impl State for ContactsPanel {
    fn view<'a>(&'a self, cache: &'a Cache) -> Element<'a, view::Message> {
        view::dashboard(
            &Menu::Contacts,
            cache,
            self.warning.as_ref(),
            view::contacts::contacts_view(
                &self.contacts,
                &self.identity_keys,
                &self.name_input,
                &self.pubkey_input,
                &self.hws.list,
                self.registering_contact,
                self.fetching_identity_key,
            ),
        )
    }

    fn update(
        &mut self,
        _daemon: Arc<dyn Daemon + Sync + Send>,
        _cache: &Cache,
        message: Message,
    ) -> Task<Message> {
        match message {
            Message::View(view::Message::Contacts(msg)) => match msg {
                view::ContactsMessage::NameEdited(name) => {
                    self.name_input.value = name;
                }
                view::ContactsMessage::PubkeyEdited(pubkey) => {
                    self.pubkey_input.value = pubkey;
                    self.pubkey_input.valid = true;
                }
                view::ContactsMessage::AddContact => {
                    if self.name_input.value.trim().is_empty() {
                        self.warning = Some(Error::Unexpected("Name cannot be empty".to_string()));
                        return Task::none();
                    }
                    let pubkey_hex = self.pubkey_input.value.trim().to_string();
                    if let Err(e) = parse_pubkey_to_xpub(&pubkey_hex) {
                        self.pubkey_input.valid = false;
                        self.warning = Some(Error::Unexpected(e));
                        return Task::none();
                    }
                    let contact = ContactSetting {
                        name: self.name_input.value.trim().to_string(),
                        pubkey: pubkey_hex,
                        registrations: Vec::new(),
                    };
                    self.contacts.push(contact);
                    self.name_input = form::Value::default();
                    self.pubkey_input = form::Value::default();
                    self.warning = None;
                    return save_contacts(
                        self.data_dir.clone(),
                        self.network,
                        self.wallet.id(),
                        self.contacts.clone(),
                    );
                }
                view::ContactsMessage::RegisterContact(contact_idx, hw_idx) => {
                    if self.registering_contact.is_some() {
                        return Task::none();
                    }
                    let contact = match self.contacts.get(contact_idx) {
                        Some(c) => c,
                        None => return Task::none(),
                    };
                    let (device, fingerprint) = match self.hws.list.get(hw_idx) {
                        Some(HardwareWallet::Supported {
                            device,
                            fingerprint,
                            ..
                        }) => (device.clone(), *fingerprint),
                        _ => return Task::none(),
                    };
                    let xpub = match parse_pubkey_to_xpub(&contact.pubkey) {
                        Ok(x) => x,
                        Err(e) => {
                            self.warning = Some(Error::Unexpected(e));
                            return Task::none();
                        }
                    };
                    self.registering_contact = Some(contact_idx);
                    self.warning = None;
                    let name = contact.name.clone();
                    return Task::perform(
                        async move {
                            let (reg_id, proof) =
                                device.register_identity_key(&name, &xpub).await?;
                            Ok(ContactRegistration {
                                device_fingerprint: fingerprint,
                                registration_id: hex::encode(reg_id.as_bytes()),
                                proof: hex::encode(proof.dangerous_as_bytes()),
                            })
                        },
                        move |res: Result<ContactRegistration, async_hwi::Error>| {
                            Message::View(view::Message::Contacts(match res {
                                Ok(reg) => view::ContactsMessage::Registered(contact_idx, reg),
                                Err(e) => view::ContactsMessage::RegistrationFailed(e.to_string()),
                            }))
                        },
                    );
                }
                view::ContactsMessage::Registered(contact_idx, registration) => {
                    self.registering_contact = None;
                    if let Some(contact) = self.contacts.get_mut(contact_idx) {
                        // Replace existing registration for same device, or add new one.
                        if let Some(existing) = contact
                            .registrations
                            .iter_mut()
                            .find(|r| r.device_fingerprint == registration.device_fingerprint)
                        {
                            *existing = registration;
                        } else {
                            contact.registrations.push(registration);
                        }
                        self.warning = None;
                        return save_contacts(
                            self.data_dir.clone(),
                            self.network,
                            self.wallet.id(),
                            self.contacts.clone(),
                        );
                    }
                }
                view::ContactsMessage::RegistrationFailed(e) => {
                    self.registering_contact = None;
                    self.warning = Some(Error::Unexpected(e));
                }
                view::ContactsMessage::Delete(i) => {
                    if i < self.contacts.len() {
                        self.contacts.remove(i);
                        return save_contacts(
                            self.data_dir.clone(),
                            self.network,
                            self.wallet.id(),
                            self.contacts.clone(),
                        );
                    }
                }
                view::ContactsMessage::ShareIdentityKey(hw_idx) => {
                    if self.fetching_identity_key {
                        return Task::none();
                    }
                    let (device, fingerprint) = match self.hws.list.get(hw_idx) {
                        Some(HardwareWallet::Supported {
                            device,
                            fingerprint,
                            ..
                        }) => (device.clone(), *fingerprint),
                        _ => return Task::none(),
                    };
                    self.fetching_identity_key = true;
                    self.warning = None;
                    return Task::perform(
                        async move {
                            let xpub = device.get_identity_key(0).await?;
                            let pubkey_hex = hex::encode(xpub.public_key.serialize());
                            Ok((fingerprint, pubkey_hex))
                        },
                        move |res: Result<(Fingerprint, String), async_hwi::Error>| {
                            Message::View(view::Message::Contacts(match res {
                                Ok((fg, pubkey)) => {
                                    view::ContactsMessage::IdentityKeyReceived(fg, pubkey)
                                }
                                Err(e) => view::ContactsMessage::IdentityKeyFailed(e.to_string()),
                            }))
                        },
                    );
                }
                view::ContactsMessage::IdentityKeyReceived(fingerprint, pubkey) => {
                    self.fetching_identity_key = false;
                    // Replace existing entry for this device, or add new one.
                    if let Some(existing) = self
                        .identity_keys
                        .iter_mut()
                        .find(|k| k.device_fingerprint == fingerprint)
                    {
                        existing.pubkey = pubkey;
                    } else {
                        self.identity_keys.push(IdentityKeySetting {
                            device_fingerprint: fingerprint,
                            pubkey,
                        });
                    }
                    self.warning = None;
                    return save_identity_keys(
                        self.data_dir.clone(),
                        self.network,
                        self.wallet.id(),
                        self.identity_keys.clone(),
                    );
                }
                view::ContactsMessage::IdentityKeyFailed(e) => {
                    self.fetching_identity_key = false;
                    self.warning = Some(Error::Unexpected(e));
                }
            },
            Message::HardwareWallets(msg) => match self.hws.update(msg) {
                Ok(cmd) => return cmd.map(Message::HardwareWallets),
                Err(e) => {
                    self.warning = Some(e.into());
                }
            },
            _ => {}
        }
        Task::none()
    }

    fn subscription(&self) -> Subscription<Message> {
        self.hws.refresh().map(Message::HardwareWallets)
    }

    fn reload(
        &mut self,
        _daemon: Arc<dyn Daemon + Sync + Send>,
        wallet: Arc<Wallet>,
    ) -> Task<Message> {
        let data_dir = self.data_dir.clone();
        let network = self.network;
        self.wallet = wallet.clone();
        self.contacts = load_contacts(&data_dir, network, &wallet);
        self.identity_keys = load_identity_keys(&data_dir, network, &wallet);
        self.hws = HardwareWallets::new(data_dir, network);
        self.warning = None;
        self.registering_contact = None;
        self.fetching_identity_key = false;
        Task::none()
    }
}
