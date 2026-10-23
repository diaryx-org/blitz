use accesskit::{Action, Node as AccessKitNode, Role};
use blitz_dom::{Document, DocumentConfig};
use blitz_html::{HtmlDocument, HtmlProvider};
use blitz_traits::events::{BlitzImeEvent, UiEvent};
use blitz_traits::shell::{ColorScheme, Viewport};
use std::sync::Arc;
use test_that::prelude::*;

#[test]
fn a_text_input_has_its_text_as_value_and_can_be_set() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input type="text" value="Ada" placeholder="Your name"></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let (_, input) = find(&tree_update.nodes, Role::TextInput);
    verify_that!(input.value(), some(eq("Ada")))?;
    verify_that!(input.placeholder(), some(eq("Your name")))?;
    verify_that!(input.supports_action(Action::Focus), eq(true))?;
    verify_that!(input.supports_action(Action::SetValue), eq(true))
}

#[test]
fn a_password_shows_only_how_long_it_is() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input type="password" value="secret"></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let (_, input) = find(&tree_update.nodes, Role::PasswordInput);
    verify_that!(input.value(), some(eq("••••••")))
}

#[test]
fn a_disabled_or_read_only_input_cannot_be_set() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input type="text" disabled><textarea readonly></textarea></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let (_, disabled) = find(&tree_update.nodes, Role::TextInput);
    verify_that!(disabled.supports_action(Action::Focus), eq(false))?;
    verify_that!(disabled.supports_action(Action::SetValue), eq(false))?;
    let (_, read_only) = find(&tree_update.nodes, Role::MultilineTextInput);
    verify_that!(read_only.is_read_only(), eq(true))?;
    verify_that!(read_only.supports_action(Action::Focus), eq(true))?;
    verify_that!(read_only.supports_action(Action::SetValue), eq(false))
}

/// What `View::accessibility_action` does for `SetValue`: focus the input,
/// select all of it, and commit the new text over the selection.
#[test]
fn text_committed_over_a_selection_of_all_replaces_the_value() -> TestResult<()> {
    let mut document = HtmlDocument::from_html(
        r#"<html><input type="text" value="Ada"></html>"#,
        default_document_config(),
    );
    document.resolve(0.0);
    let (id, _) = find(&document.build_accessibility_tree().nodes, Role::TextInput);
    let node = blitz_dom::NodeId::from_u64(id.0);

    document.set_focus_to(node);
    verify_that!(document.select_all_in_text_input(node), eq(true))?;
    document.handle_ui_event(UiEvent::Ime(BlitzImeEvent::Commit("Grace".into())));
    document.resolve(0.0);

    let tree_update = document.build_accessibility_tree();
    let (_, input) = find(&tree_update.nodes, Role::TextInput);
    verify_that!(input.value(), some(eq("Grace")))
}

fn find(
    nodes: &[(accesskit::NodeId, AccessKitNode)],
    role: Role,
) -> (accesskit::NodeId, AccessKitNode) {
    nodes
        .iter()
        .find(|(_, node)| node.role() == role)
        .cloned()
        .unwrap_or_else(|| panic!("no {role:?} in the tree"))
}

fn default_document_config() -> DocumentConfig {
    DocumentConfig {
        viewport: Some(Viewport::new(800, 600, 1.0, ColorScheme::Light)),
        html_parser_provider: Some(Arc::new(HtmlProvider) as _),
        ..Default::default()
    }
}
