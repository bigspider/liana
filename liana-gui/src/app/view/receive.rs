use std::collections::{HashMap, HashSet};

use iced::{
    alignment::Horizontal,
    widget::{
        checkbox,
        qr_code::{self, QRCode},
        scrollable, Button, Space,
    },
    Alignment, Length,
};

use liana::miniscript::bitcoin::{
    self,
    bip32::{ChildNumber, Fingerprint},
    Address,
};

use liana_ui::{
    component::{
        button, card, form,
        text::{self, *},
    },
    icon, theme,
    widget::*,
};

use crate::{
    app::{
        error::Error,
        view::{hw, label, warning::warn},
    },
    hw::{HardwareWallet, UnsupportedReason},
};

use liana_ui::component::hw as hw_component;

use super::message::Message;

fn address_card<'a>(
    row_index: usize,
    address: &'a bitcoin::Address,
    labels: &'a HashMap<String, String>,
    labels_editing: &'a HashMap<String, form::Value<String>>,
    signed_address: Option<&'a str>,
) -> Container<'a, Message> {
    let addr = address.to_string();
    let display_addr = signed_address.unwrap_or(&addr);
    card::simple(
        Column::new()
            .push(if let Some(label) = labels_editing.get(&addr) {
                label::label_editing(vec![addr.clone()], label, text::P1_SIZE)
            } else {
                label::label_editable(vec![addr.clone()], labels.get(&addr), text::P1_SIZE)
            })
            .push(
                Row::new()
                    .push(
                        Container::new(
                            scrollable(
                                Column::new()
                                    .push(Space::with_height(Length::Fixed(10.0)))
                                    .push(
                                        p2_regular(display_addr)
                                            .small()
                                            .style(theme::text::secondary),
                                    )
                                    // Space between the address and the scrollbar
                                    .push(Space::with_height(Length::Fixed(10.0))),
                            )
                            .direction(
                                scrollable::Direction::Horizontal(
                                    scrollable::Scrollbar::new().width(2).scroller_width(2),
                                ),
                            ),
                        )
                        .width(Length::Fill),
                    )
                    .push(
                        Button::new(icon::clipboard_icon().style(theme::text::secondary))
                            .on_press(Message::Clipboard(display_addr.to_string()))
                            .style(theme::button::transparent_border),
                    )
                    .align_y(Alignment::Center),
            )
            .push(
                Row::new()
                    .push(
                        button::secondary(None, "Verify on hardware device")
                            .on_press(Message::Select(row_index)),
                    )
                    .push(Space::with_width(Length::Fill))
                    .push(
                        button::secondary(None, "Show QR Code")
                            .on_press(Message::ShowQrCode(row_index)),
                    ),
            )
            .spacing(10),
    )
}

#[allow(clippy::too_many_arguments)]
pub fn receive<'a>(
    addresses: &'a [bitcoin::Address],
    labels: &'a HashMap<String, String>,
    prev_addresses: &'a [bitcoin::Address],
    prev_labels: &'a HashMap<String, String>,
    show_prev_addresses: bool,
    selected: &'a HashSet<bitcoin::Address>,
    labels_editing: &'a HashMap<String, form::Value<String>>,
    is_last_page: bool,
    processing: bool,
    sign_with_identity: bool,
    identity_hws: Option<&'a [HardwareWallet]>,
    selected_identity_hw: Option<usize>,
    signed_addresses: &'a HashMap<bitcoin::Address, String>,
    signing_address: bool,
) -> Element<'a, Message> {
    // Number of start and end address characters to show in collapsed view.
    const NUM_ADDR_CHARS: usize = 16;
    let mut addresses_count = 0; // for counting number of new addresses generated
    Column::new()
        .push(
            Row::new()
                .align_y(Alignment::Center)
                .push(Container::new(h3("Receive")).width(Length::Fill))
                .push({
                    let (icon, label) = (Some(icon::plus_icon()), "Generate address");
                    if addresses.is_empty() {
                        button::primary(icon, label)
                    } else {
                        button::secondary(icon, label)
                    }
                    .on_press(Message::NextReceiveAddress)
                }),
        )
        .push(text("Always generate a new address for each deposit."))
        .push(
            Container::new(
                checkbox("Sign address with your identity key", sign_with_identity)
                    .on_toggle(Message::ToggleSignAddressIdentity),
            )
            .padding(5),
        )
        .push_maybe(identity_hws.map(|hws| {
            Column::new()
                .spacing(10)
                .push(text("Select device to sign with:").width(Length::Fill))
                .push(
                    hws.iter()
                        .enumerate()
                        .fold(Column::new().spacing(5), |col, (i, hw_item)| {
                            let is_selected = selected_identity_hw == Some(i);
                            let mut bttn = Button::new(match hw_item {
                                HardwareWallet::Supported {
                                    kind,
                                    version,
                                    fingerprint,
                                    alias,
                                    ..
                                } => hw_component::supported_hardware_wallet(
                                    kind,
                                    version.as_ref(),
                                    fingerprint,
                                    alias.as_ref(),
                                ),
                                HardwareWallet::Unsupported {
                                    version,
                                    kind,
                                    reason,
                                    ..
                                } => match reason {
                                    UnsupportedReason::NotPartOfWallet(fg) => {
                                        hw_component::unrelated_hardware_wallet(
                                            kind.to_string(),
                                            version.as_ref(),
                                            fg,
                                        )
                                    }
                                    UnsupportedReason::WrongNetwork => {
                                        hw_component::wrong_network_hardware_wallet(
                                            kind.to_string(),
                                            version.as_ref(),
                                        )
                                    }
                                    _ => hw_component::unsupported_hardware_wallet(
                                        kind.to_string(),
                                        version.as_ref(),
                                    ),
                                },
                                HardwareWallet::Locked {
                                    kind, pairing_code, ..
                                } => hw_component::locked_hardware_wallet(
                                    kind,
                                    pairing_code.as_ref(),
                                ),
                            })
                            .style(if is_selected {
                                theme::button::primary
                            } else {
                                theme::button::secondary
                            })
                            .width(Length::Fill);
                            if hw_item.is_supported() {
                                bttn = bttn.on_press(Message::SelectIdentityHardwareWallet(i));
                            }
                            col.push(bttn)
                        }),
                )
        }))
        .push_maybe(
            signing_address.then_some(text("Signing address...").style(theme::text::secondary)),
        )
        .push(
            Row::new()
                .spacing(10)
                .push(addresses.iter().enumerate().rev().fold(
                    // iterate starting from most recently generated
                    Column::new().spacing(10).width(Length::Fill),
                    |col, (i, address)| {
                        addresses_count += 1;
                        let signed = signed_addresses.get(address).map(|s| s.as_str());
                        col.push(address_card(i, address, labels, labels_editing, signed))
                    },
                )),
        )
        .push_maybe(
            (!prev_addresses.is_empty()).then_some(
                Container::new(
                    Button::new(
                        Row::new()
                            .align_y(Alignment::Center)
                            .push(
                                p1_bold("Previously generated addresses still awaiting deposit")
                                    .width(Length::Fill),
                            )
                            .push(if show_prev_addresses {
                                icon::collapsed_icon()
                            } else {
                                icon::collapse_icon()
                            }),
                    )
                    .on_press(Message::ToggleShowPreviousAddresses)
                    .padding(20)
                    .width(Length::Fill)
                    .style(theme::button::transparent_border),
                )
                .style(theme::card::simple),
            ),
        )
        .push_maybe(show_prev_addresses.then_some(Row::new().spacing(10).push(
            prev_addresses.iter().enumerate().fold(
                // prev addresses are already ordered in descending order
                Column::new().spacing(10).width(Length::Fill),
                |col, (i, address)| {
                    col.push(if !selected.contains(address) {
                        Button::new(
                            Row::new()
                                .spacing(10)
                                .push(
                                    {
                                        let addr = address.to_string();
                                        let addr_len = addr.chars().count();
                                        Container::new(
                                            p2_regular(if addr_len > 2 * NUM_ADDR_CHARS {
                                                format!(
                                                    "{}...{}",
                                                    addr.chars()
                                                        .take(NUM_ADDR_CHARS)
                                                        .collect::<String>(),
                                                    addr.chars()
                                                        .skip(addr_len - NUM_ADDR_CHARS)
                                                        .collect::<String>(),
                                                )
                                            } else {
                                                addr
                                            })
                                            .small()
                                            .style(theme::text::secondary),
                                        )
                                    }
                                    .padding(10)
                                    .width(Length::Fixed(350.0)),
                                )
                                .push(
                                    Container::new(
                                        scrollable(
                                            Column::new()
                                                .push(Space::with_height(Length::Fixed(10.0)))
                                                .push(
                                                    text(
                                                        prev_labels
                                                            .get(&address.to_string())
                                                            .cloned()
                                                            .unwrap_or_default(),
                                                    )
                                                    .small()
                                                    .style(theme::text::secondary),
                                                )
                                                // Space between the label and the scrollbar
                                                .push(Space::with_height(Length::Fixed(10.0))),
                                        )
                                        .direction(
                                            scrollable::Direction::Horizontal(
                                                scrollable::Scrollbar::new()
                                                    .width(2)
                                                    .scroller_width(2),
                                            ),
                                        ),
                                    )
                                    .padding(10)
                                    .width(Length::Fill),
                                )
                                .align_y(Alignment::Center),
                        )
                        .on_press(Message::SelectAddress(address.clone()))
                        .style(theme::button::secondary)
                    } else {
                        // Continue the row index from those of generated addresses above.
                        Button::new(address_card(
                            addresses_count + i,
                            address,
                            prev_labels,
                            labels_editing,
                            None,
                        ))
                        .padding(0) // so that button & card borders match
                        .on_press(Message::SelectAddress(address.clone()))
                        .style(theme::button::transparent_border)
                    })
                },
            ),
        )))
        .push_maybe(
            (!is_last_page && show_prev_addresses).then_some(
                Container::new(
                    Button::new(
                        text(if processing {
                            "Fetching ..."
                        } else {
                            "See more"
                        })
                        .width(Length::Fill)
                        .align_x(Horizontal::Center),
                    )
                    .width(Length::Fill)
                    .padding(15)
                    .style(theme::button::transparent_border)
                    .on_press_maybe((!processing).then_some(Message::Next)),
                )
                .width(Length::Fill)
                .style(theme::card::simple),
            ),
        )
        .spacing(20)
        .into()
}

pub fn verify_address_modal<'a>(
    warning: Option<&Error>,
    hws: &'a [HardwareWallet],
    chosen_hws: &HashSet<Fingerprint>,
    address: &Address,
    derivation_index: &ChildNumber,
) -> Element<'a, Message> {
    Column::new()
        .push_maybe(warning.map(|w| warn(Some(w))))
        .push(card::simple(
            Column::new()
                .push(
                    Column::new()
                        .push(
                            Column::new()
                                .push(
                                    Row::new()
                                        .width(Length::Fill)
                                        .align_y(Alignment::Center)
                                        .push(
                                            Container::new(text("Address:").bold())
                                                .width(Length::Fill),
                                        )
                                        .push(
                                            Row::new()
                                                .align_y(Alignment::Center)
                                                .push(Container::new(
                                                    text(address.to_string()).small(),
                                                ))
                                                .push(
                                                    Button::new(icon::clipboard_icon())
                                                        .on_press(Message::Clipboard(
                                                            address.to_string(),
                                                        ))
                                                        .style(theme::button::transparent_border),
                                                )
                                                .width(Length::Shrink),
                                        ),
                                )
                                .push(
                                    Row::new()
                                        .width(Length::Fill)
                                        .align_y(Alignment::Center)
                                        .push(
                                            Container::new(text("Derivation index:").bold())
                                                .width(Length::Fill),
                                        )
                                        .push(
                                            Container::new(
                                                text(derivation_index.to_string()).small(),
                                            )
                                            .width(Length::Shrink),
                                        ),
                                )
                                .spacing(5),
                        )
                        .push(text("Select device to verify address on:").width(Length::Fill))
                        .spacing(10)
                        .push(hws.iter().enumerate().fold(
                            Column::new().spacing(10),
                            |col, (i, hw)| {
                                col.push(hw::hw_list_view_verify_address(
                                    i,
                                    hw,
                                    if let HardwareWallet::Supported { fingerprint, .. } = hw {
                                        chosen_hws.contains(fingerprint)
                                    } else {
                                        false
                                    },
                                ))
                            },
                        ))
                        .width(Length::Fill),
                )
                .spacing(20)
                .width(Length::Fill)
                .align_x(Alignment::Center),
        ))
        .width(Length::Fill)
        .max_width(750)
        .into()
}

pub fn qr_modal<'a>(qr: &'a qr_code::Data, address: &'a String) -> Element<'a, Message> {
    Column::new()
        .push(
            Row::new()
                .push(Space::with_width(Length::Fill))
                .push(
                    Container::new(QRCode::<liana_ui::theme::Theme>::new(qr).cell_size(8))
                        .padding(10),
                )
                .push(Space::with_width(Length::Fill)),
        )
        .push(Space::with_height(Length::Fixed(15.0)))
        .push(Container::new(text(address).size(15)).center_x(Length::Fill))
        .width(Length::Fill)
        .max_width(400)
        .into()
}
