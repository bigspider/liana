use iced::{widget::Space, Length};

use liana::miniscript::bitcoin::bip32::Fingerprint;
use liana_ui::{
    component::{button, card, form, text, text::*},
    icon, theme,
    widget::*,
};

use crate::{
    app::{
        settings::{ContactSetting, IdentityKeySetting},
        view::message::*,
    },
    hw::HardwareWallet,
};

pub fn contacts_view<'a>(
    contacts: &'a [ContactSetting],
    identity_keys: &'a [IdentityKeySetting],
    name_input: &'a form::Value<String>,
    pubkey_input: &'a form::Value<String>,
    hws: &'a [HardwareWallet],
    registering_contact: Option<usize>,
    fetching_identity_key: bool,
) -> Element<'a, Message> {
    let mut col = Column::new().spacing(20);

    col = col.push(h3("Contacts"));

    // "Share my identity key" section at the top.
    col = col.push(my_identity_key_section(
        identity_keys,
        hws,
        fetching_identity_key,
    ));

    // Input form for adding a new contact (without registration).
    let name_form = form::Form::new("Contact name", name_input, |s| {
        Message::Contacts(ContactsMessage::NameEdited(s))
    })
    .size(text::P1_SIZE)
    .padding(10);

    let pubkey_form = form::Form::new_trimmed("66-character hex pubkey", pubkey_input, |s| {
        Message::Contacts(ContactsMessage::PubkeyEdited(s))
    })
    .size(text::P1_SIZE)
    .padding(10);

    let can_add = !name_input.value.trim().is_empty() && !pubkey_input.value.trim().is_empty();

    let mut add_btn = button::primary(None, "Add contact");
    if can_add {
        add_btn = add_btn.on_press(Message::Contacts(ContactsMessage::AddContact));
    }

    let add_section = card::simple(
        Column::new()
            .spacing(15)
            .push(h4_bold("Add a new contact"))
            .push(
                Column::new()
                    .spacing(10)
                    .push(p1_bold("Name"))
                    .push(name_form),
            )
            .push(
                Column::new()
                    .spacing(10)
                    .push(p1_bold("Identity public key"))
                    .push(pubkey_form),
            )
            .push(add_btn),
    )
    .width(Length::Fill);

    col = col.push(add_section);

    // Existing contacts list.
    if !contacts.is_empty() {
        col = col.push(h4_bold("Contacts"));
        for (i, contact) in contacts.iter().enumerate() {
            col = col.push(contact_card(i, contact, hws, registering_contact));
        }
    }

    col = col.push(Space::with_height(Length::Fixed(50.0)));

    col.into()
}

fn my_identity_key_section<'a>(
    identity_keys: &'a [IdentityKeySetting],
    hws: &'a [HardwareWallet],
    fetching_identity_key: bool,
) -> Element<'a, Message> {
    let mut content = Column::new().spacing(15).push(h4_bold("My identity key"));

    // Show already-known identity keys.
    if !identity_keys.is_empty() {
        for ik in identity_keys {
            let truncated = if ik.pubkey.len() > 20 {
                format!(
                    "{}...{}",
                    &ik.pubkey[..10],
                    &ik.pubkey[ik.pubkey.len() - 10..]
                )
            } else {
                ik.pubkey.clone()
            };
            content = content.push(
                Row::new()
                    .spacing(10)
                    .align_y(iced::Alignment::Center)
                    .push(p1_bold(format!("Device {}", ik.device_fingerprint)))
                    .push(
                        button::secondary(None, "Copy")
                            .on_press(Message::Clipboard(ik.pubkey.clone()))
                            .width(Length::Shrink),
                    )
                    .push(p2_regular(truncated)),
            );
        }
    }

    // Hardware wallet buttons to fetch identity key.
    let hw_section = hws
        .iter()
        .enumerate()
        .fold(Column::new().spacing(5), |col, (hw_idx, h)| {
            let already_fetched = if let HardwareWallet::Supported { fingerprint, .. } = h {
                identity_keys
                    .iter()
                    .any(|k| k.device_fingerprint == *fingerprint)
            } else {
                false
            };

            if already_fetched {
                return col;
            }

            let mut bttn = Button::new(match h {
                HardwareWallet::Supported {
                    kind,
                    version,
                    fingerprint,
                    alias,
                    ..
                } => {
                    if fetching_identity_key {
                        liana_ui::component::hw::processing_hardware_wallet(
                            kind,
                            version.as_ref(),
                            fingerprint,
                            alias.as_ref(),
                        )
                    } else {
                        liana_ui::component::hw::supported_hardware_wallet(
                            kind,
                            version.as_ref(),
                            fingerprint,
                            alias.as_ref(),
                        )
                    }
                }
                HardwareWallet::Unsupported {
                    version,
                    kind,
                    reason,
                    ..
                } => match reason {
                    crate::hw::UnsupportedReason::NotPartOfWallet(fg) => {
                        liana_ui::component::hw::unrelated_hardware_wallet(
                            kind.to_string(),
                            version.as_ref(),
                            fg,
                        )
                    }
                    crate::hw::UnsupportedReason::WrongNetwork => {
                        liana_ui::component::hw::wrong_network_hardware_wallet(
                            kind.to_string(),
                            version.as_ref(),
                        )
                    }
                    _ => liana_ui::component::hw::unsupported_hardware_wallet(
                        kind.to_string(),
                        version.as_ref(),
                    ),
                },
                HardwareWallet::Locked {
                    kind, pairing_code, ..
                } => liana_ui::component::hw::locked_hardware_wallet(kind, pairing_code.as_ref()),
            })
            .style(theme::button::secondary)
            .width(Length::Fill);

            if !fetching_identity_key && h.is_supported() {
                bttn = bttn.on_press(Message::Contacts(ContactsMessage::ShareIdentityKey(hw_idx)));
            }

            col.push(bttn)
        });

    content = content
        .push(p2_regular(
            "Share your identity key from a connected signing device:",
        ))
        .push(hw_section);

    card::simple(content).width(Length::Fill).into()
}

fn contact_card<'a>(
    contact_idx: usize,
    contact: &'a ContactSetting,
    hws: &'a [HardwareWallet],
    registering_contact: Option<usize>,
) -> Element<'a, Message> {
    let is_registered = !contact.registrations.is_empty();
    let is_registering = registering_contact == Some(contact_idx);

    let truncated_pubkey = if contact.pubkey.len() > 20 {
        format!(
            "{}...{}",
            &contact.pubkey[..10],
            &contact.pubkey[contact.pubkey.len() - 10..]
        )
    } else {
        contact.pubkey.clone()
    };

    let status_text = if is_registered {
        p2_regular(format!(
            "Registered on {} device{}",
            contact.registrations.len(),
            if contact.registrations.len() == 1 {
                ""
            } else {
                "s"
            }
        ))
    } else {
        p2_regular("Unregistered")
    };

    let registered_fingerprints: Vec<Fingerprint> = contact
        .registrations
        .iter()
        .map(|r| r.device_fingerprint)
        .collect();

    // Hardware wallet buttons for registration.
    let hw_section = hws
        .iter()
        .enumerate()
        .fold(Column::new().spacing(5), |col, (hw_idx, h)| {
            let already_registered = if let HardwareWallet::Supported { fingerprint, .. } = h {
                registered_fingerprints.contains(fingerprint)
            } else {
                false
            };

            let mut bttn = Button::new(match h {
                HardwareWallet::Supported {
                    kind,
                    version,
                    fingerprint,
                    alias,
                    ..
                } => {
                    if is_registering {
                        liana_ui::component::hw::processing_hardware_wallet(
                            kind,
                            version.as_ref(),
                            fingerprint,
                            alias.as_ref(),
                        )
                    } else if already_registered {
                        liana_ui::component::hw::registration_success_hardware_wallet(
                            kind,
                            version.as_ref(),
                            fingerprint,
                            alias.as_ref(),
                        )
                    } else {
                        liana_ui::component::hw::supported_hardware_wallet(
                            kind,
                            version.as_ref(),
                            fingerprint,
                            alias.as_ref(),
                        )
                    }
                }
                HardwareWallet::Unsupported {
                    version,
                    kind,
                    reason,
                    ..
                } => match reason {
                    crate::hw::UnsupportedReason::NotPartOfWallet(fg) => {
                        liana_ui::component::hw::unrelated_hardware_wallet(
                            kind.to_string(),
                            version.as_ref(),
                            fg,
                        )
                    }
                    crate::hw::UnsupportedReason::WrongNetwork => {
                        liana_ui::component::hw::wrong_network_hardware_wallet(
                            kind.to_string(),
                            version.as_ref(),
                        )
                    }
                    _ => liana_ui::component::hw::unsupported_hardware_wallet(
                        kind.to_string(),
                        version.as_ref(),
                    ),
                },
                HardwareWallet::Locked {
                    kind, pairing_code, ..
                } => liana_ui::component::hw::locked_hardware_wallet(kind, pairing_code.as_ref()),
            })
            .style(theme::button::secondary)
            .width(Length::Fill);

            if !is_registering && registering_contact.is_none() && h.is_supported() {
                bttn = bttn.on_press(Message::Contacts(ContactsMessage::RegisterContact(
                    contact_idx,
                    hw_idx,
                )));
            }

            col.push(bttn)
        });

    card::simple(
        Column::new()
            .spacing(10)
            .push(
                Row::new()
                    .spacing(10)
                    .align_y(iced::Alignment::Center)
                    .push(
                        Column::new()
                            .spacing(5)
                            .push(p1_bold(&contact.name))
                            .push(p2_regular(truncated_pubkey))
                            .push(status_text)
                            .width(Length::Fill),
                    )
                    .push(
                        button::secondary(Some(icon::trash_icon()), "Delete")
                            .on_press(Message::Contacts(ContactsMessage::Delete(contact_idx)))
                            .width(Length::Shrink),
                    ),
            )
            .push(p2_regular("Register with a signing device:"))
            .push(hw_section),
    )
    .width(Length::Fill)
    .into()
}
