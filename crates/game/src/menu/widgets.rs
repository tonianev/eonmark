//! The menus' shared widgets (M6 plan §6.8): the button, the labelled row
//! of option buttons and the statistics row that the New Game selector
//! (`main_menu`, `new_game`) and the game-over screen (`crate::outcome`)
//! build from, so M7's Kenney reskin changes this one module.
//!
//! Every widget is a plain `bevy_ui` node: a button is a `Button` sized
//! like a command-card button (`command_card::CARD_BUTTON_PX`) on
//! `palette::HUD_BUTTON` with one text child that ignores picking; its
//! hover and pressed colours come from [`button_colors`]. Clicks are
//! `Pointer<Click>` observers the caller attaches, as the HUD's Stop button
//! does; `Interaction` only drives the colours.

use bevy::ecs::observer::IntoEntityObserver;
use bevy::prelude::*;

use crate::command_card::{CARD_BUTTON_PX, CARD_GAP_PX};
use crate::palette;

/// Width of an option row's label column, logical px.
pub const ROW_LABEL_PX: f32 = 150.0;

/// A text that ignores picking, in the HUD font at `size` px and the HUD
/// text colour.
pub fn label(value: &str, size: f32) -> impl Bundle {
    (
        Text::new(value),
        crate::fonts::hud_font(size),
        TextColor(palette::HUD_TEXT),
        Pickable::IGNORE,
    )
}

/// Spawn under `parent` a button carrying `marker`, named `name`, whose one
/// child is `text` (usually [`label`], with the caller's own marker when it
/// fills the text later); the caller attaches its click observer to the
/// returned entity.
pub fn spawn_button<'a>(
    parent: &'a mut ChildSpawnerCommands,
    name: String,
    marker: impl Bundle,
    text: impl Bundle,
) -> EntityCommands<'a> {
    let mut button = parent.spawn((
        Name::new(name),
        marker,
        Button,
        Node {
            width: Val::Px(CARD_BUTTON_PX.x),
            height: Val::Px(CARD_BUTTON_PX.y),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(palette::HUD_BUTTON),
    ));
    button.with_children(|b| {
        b.spawn(text);
    });
    button
}

/// Hover and pressed colours (`palette::HUD_BUTTON*`) for the buttons
/// marked `M`.
pub fn button_colors<M: Component>(
    mut buttons: Query<(&Interaction, &mut BackgroundColor), (Changed<Interaction>, With<M>)>,
) {
    for (interaction, mut background) in &mut buttons {
        background.0 = button_color(*interaction);
    }
}

/// A button's background for `interaction`.
pub fn button_color(interaction: Interaction) -> Color {
    match interaction {
        Interaction::Pressed => palette::HUD_BUTTON_PRESSED,
        Interaction::Hovered => palette::HUD_BUTTON_HOVER,
        Interaction::None => palette::HUD_BUTTON,
    }
}

/// Spawn under `menu` one row: the label `title` in a fixed-width column,
/// then one button per `(marker, name)` in `options`, each observed by
/// `on_click`.
pub fn option_row<B: Bundle, M>(
    menu: &mut ChildSpawnerCommands,
    title: &str,
    size: f32,
    options: Vec<(B, String)>,
    on_click: impl IntoEntityObserver<M> + Clone,
) {
    menu.spawn((
        Node {
            flex_direction: FlexDirection::Row,
            column_gap: Val::Px(CARD_GAP_PX),
            align_items: AlignItems::Center,
            ..default()
        },
        Pickable::IGNORE,
    ))
    .with_children(|row| {
        row.spawn((
            Node {
                width: Val::Px(ROW_LABEL_PX),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|l| {
            l.spawn(label(title, size));
        });
        for (marker, name) in options {
            spawn_button(row, format!("Option {name}"), marker, label(&name, size))
                .observe(on_click.clone());
        }
    });
}

/// Spawn under `grid` (a `Display::Grid` node) one statistics row: each
/// cell a text at `size` px in its colour.
pub fn stat_row(grid: &mut ChildSpawnerCommands, cells: &[(String, Color)], size: f32) {
    for (text, color) in cells {
        grid.spawn((
            Text::new(text.clone()),
            crate::fonts::hud_font(size),
            TextColor(*color),
            Pickable::IGNORE,
        ));
    }
}
