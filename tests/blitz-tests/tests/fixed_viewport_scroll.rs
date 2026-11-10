//! A `position: fixed` box whose containing block is the root element stays
//! where the viewport puts it while the document scrolls: drawn there, hit
//! there, and measured there by `get_client_bounding_rect`. In an HTML
//! document and in a dioxus-native one, whose root element has a node above
//! it that is not an element, which a test of "no containing block" missed.

use blitz_dom::Point;
use blitz_test_harness::Harness;
use blitz_traits::node_id::NodeId;
use dioxus::prelude::*;

const PAGE: &str = "<html><body style='margin:0'>\
    <div id=tall style='height:3000px'></div>\
    <div id=hoisted style='position:fixed;left:10px;top:10px;width:100px;height:40px;z-index:5'></div>\
    <div id=plain style='position:fixed;left:10px;top:100px;width:100px;height:40px'></div>\
    </body></html>";

#[allow(non_snake_case)]
fn Page() -> Element {
    rsx! {
        div { id: "tall", style: "height:3000px" }
        div { id: "hoisted", style: "position:fixed;left:10px;top:10px;width:100px;height:40px;z-index:5" }
        div { id: "plain", style: "position:fixed;left:10px;top:100px;width:100px;height:40px" }
    }
}

fn scrolled<D: blitz_dom::Document>(harness: &mut Harness<D>) {
    harness.set_viewport_size(800, 600);
    harness.pump();
    harness
        .base_mut()
        .set_viewport_scroll(Point { x: 0.0, y: 500.0 });
    harness.pump();
}

fn client_y<D: blitz_dom::Document>(harness: &Harness<D>, id: NodeId) -> f64 {
    harness.base().get_client_bounding_rect(id).unwrap().y
}

fn assert_fixed<D: blitz_dom::Document>(mut harness: Harness<D>, what: &str) {
    scrolled(&mut harness);
    let (hoisted, plain) = (harness.node("#hoisted"), harness.node("#plain"));
    let root = harness.base().root_element().id;
    assert!(
        harness.base().get_node(root).unwrap().is_root_element(),
        "{what}"
    );
    assert!(
        harness
            .base()
            .get_node(hoisted)
            .unwrap()
            .is_fixed_to_viewport(),
        "{what}"
    );
    assert_eq!(
        client_y(&harness, hoisted),
        10.0,
        "{what}: measured where it is shown"
    );
    assert_eq!(
        client_y(&harness, plain),
        100.0,
        "{what}: measured where it is shown"
    );
    // `hit` takes document coordinates: the window's (15, 15) is (15, 515).
    assert_eq!(
        harness.hit_node(15.0, 515.0),
        hoisted,
        "{what}: a press lands on it"
    );
    assert_eq!(
        harness.hit_node(15.0, 605.0),
        plain,
        "{what}: a press lands on it"
    );
    // In the document, it is where the viewport has scrolled to.
    assert_eq!(harness.layout_rect("#hoisted").y, 510.0, "{what}");
}

#[test]
fn a_fixed_box_stays_put_in_a_scrolled_html_document() {
    assert_fixed(Harness::from_html(PAGE), "html");
}

#[test]
fn a_fixed_box_stays_put_in_a_scrolled_dioxus_document() {
    assert_fixed(Harness::from_component(Page), "dioxus");
}
