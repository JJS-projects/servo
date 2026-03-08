/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/. */

use app_units::Au;
use euclid::{Point2D, Rect, Size2D};
use html5ever::{local_name, ns};
use layout_api::wrapper_traits::{ThreadSafeLayoutElement, ThreadSafeLayoutNode};
use layout_api::{LayoutElementType, LayoutNodeType};
use rustc_hash::FxHashMap;
use script::layout_dom::ServoThreadSafeLayoutNode;
use style::color::{AbsoluteColor, ColorSpace};
use style::computed_values::visibility::T as Visibility;
use style::computed_values::white_space_collapse::T as WhiteSpaceCollapseValue;
use style::properties::ComputedValues;
use style::values::computed::TextDecorationLine;
use style::values::computed::font::FontStyle;
use style_traits::CSSPixel;

use crate::fragment_tree::{ContainingBlockManager, Fragment, FragmentTree};
use crate::geom::PhysicalRect;
use crate::style_ext::{Display, DisplayGeneratingBox, DisplayInside, DisplayLayoutInternal, DisplayOutside};

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalLayoutNode {
    pub kind: TerminalNodeKind,
    pub rect: Option<Rect<f32, CSSPixel>>,
    pub style: Option<TerminalCssProperties>,
    pub children: Vec<TerminalLayoutNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TerminalNodeKind {
    Document,
    Element(TerminalElementData),
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalElementData {
    pub tag_name: String,
    pub role: TerminalElementRole,
    pub href: Option<String>,
    pub src: Option<String>,
    pub alt: Option<String>,
    pub value: Option<String>,
    pub title: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalElementRole {
    Generic,
    Link,
    Button,
    Input,
    TextArea,
    Image,
    Select,
    IFrame,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TerminalColor {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TerminalDisplay {
    None,
    Contents,
    Inline,
    Block,
    InlineBlock,
    FlowRoot,
    Flex,
    InlineFlex,
    Grid,
    InlineGrid,
    Table,
    InlineTable,
    TableCaption,
    TableCell,
    TableRow,
    TableRowGroup,
    TableColumn,
    TableColumnGroup,
    TableHeaderGroup,
    TableFooterGroup,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalCssProperties {
    pub display: TerminalDisplay,
    pub visible: bool,
    pub color: TerminalColor,
    pub background_color: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub overline: bool,
    pub line_through: bool,
    pub preserve_whitespace: bool,
}

#[derive(Clone, Debug)]
struct FragmentMetadata {
    rect: Rect<f32, CSSPixel>,
    style: TerminalCssProperties,
    fragment_count: usize,
}

pub fn build_terminal_layout_tree(
    root: ServoThreadSafeLayoutNode<'_>,
    fragment_tree: &FragmentTree,
) -> Option<embedder_traits::TerminalLayoutNode> {
    // Servo's fragment tree gives us positioned boxes, but not a lossless per-fragment text
    // payload after shaping. Walk the DOM/layout tree for semantics and attach fragment-derived
    // geometry/style metadata by opaque node id.
    let metadata = collect_fragment_metadata_from_roots(
        &fragment_tree.root_fragments,
        fragment_tree.initial_containing_block,
    );
    build_terminal_layout_node(root, &metadata).map(into_public_layout_node)
}

fn build_terminal_layout_node(
    node: ServoThreadSafeLayoutNode<'_>,
    metadata: &FxHashMap<usize, FragmentMetadata>,
) -> Option<TerminalLayoutNode> {
    let mut children = Vec::new();
    for child in node.children() {
        if let Some(child) = build_terminal_layout_node(child, metadata) {
            children.push(child);
        }
    }

    let metadata = metadata.get(&node.opaque().id());
    let kind = classify_node(node);

    if metadata.is_none() && children.is_empty() {
        return match kind {
            TerminalNodeKind::Text(ref text) if !text.is_empty() => None,
            TerminalNodeKind::Document => Some(TerminalLayoutNode {
                kind,
                rect: None,
                style: None,
                children,
            }),
            _ => None,
        };
    }

    Some(TerminalLayoutNode {
        kind,
        rect: metadata.map(|entry| entry.rect),
        style: metadata.map(|entry| entry.style.clone()),
        children,
    })
}

fn classify_node(node: ServoThreadSafeLayoutNode<'_>) -> TerminalNodeKind {
    match node.type_id() {
        Some(LayoutNodeType::Text) => TerminalNodeKind::Text(node.text_content().into_owned()),
        Some(LayoutNodeType::Element(_)) => {
            let element = node
                .as_element()
                .expect("layout element nodes should expose their element wrapper");
            let tag_name = element.get_local_name().to_string();
            let href = element
                .get_attr(&ns!(), &local_name!("href"))
                .map(ToOwned::to_owned);
            let src = element
                .get_attr(&ns!(), &local_name!("src"))
                .or_else(|| element.get_attr(&ns!(), &local_name!("data")))
                .map(ToOwned::to_owned);
            let alt = element
                .get_attr(&ns!(), &local_name!("alt"))
                .map(ToOwned::to_owned);
            let value = element
                .get_attr(&ns!(), &local_name!("value"))
                .map(ToOwned::to_owned);
            let title = element
                .get_attr(&ns!(), &local_name!("title"))
                .map(ToOwned::to_owned);
            let role = classify_element_role(&tag_name, node.type_id());

            TerminalNodeKind::Element(TerminalElementData {
                tag_name,
                role,
                href,
                src,
                alt,
                value,
                title,
            })
        },
        _ => TerminalNodeKind::Document,
    }
}

fn classify_element_role(tag_name: &str, type_id: Option<LayoutNodeType>) -> TerminalElementRole {
    if tag_name == "a" {
        return TerminalElementRole::Link;
    }

    if tag_name == "button" {
        return TerminalElementRole::Button;
    }

    match type_id {
        Some(LayoutNodeType::Element(LayoutElementType::HTMLInputElement)) => {
            TerminalElementRole::Input
        },
        Some(LayoutNodeType::Element(LayoutElementType::HTMLTextAreaElement)) => {
            TerminalElementRole::TextArea
        },
        Some(LayoutNodeType::Element(LayoutElementType::HTMLImageElement)) |
        Some(LayoutNodeType::Element(LayoutElementType::HTMLObjectElement)) |
        Some(LayoutNodeType::Element(LayoutElementType::SVGImageElement)) => {
            TerminalElementRole::Image
        },
        Some(LayoutNodeType::Element(LayoutElementType::HTMLSelectElement)) => {
            TerminalElementRole::Select
        },
        Some(LayoutNodeType::Element(LayoutElementType::HTMLIFrameElement)) => {
            TerminalElementRole::IFrame
        },
        _ => TerminalElementRole::Generic,
    }
}

fn collect_fragment_metadata_from_roots(
    root_fragments: &[Fragment],
    initial_containing_block: PhysicalRect<Au>,
) -> FxHashMap<usize, FragmentMetadata> {
    let mut metadata = FxHashMap::default();
    let containing_blocks = ContainingBlockManager {
        for_non_absolute_descendants: &initial_containing_block,
        for_absolute_descendants: None,
        for_absolute_and_fixed_descendants: &initial_containing_block,
    };

    for fragment in root_fragments {
        fragment.find(&containing_blocks, 0, &mut |fragment, _level, containing_block| {
            let Some(base) = fragment.base() else {
                return None::<()>;
            };
            let Some(tag) = base.tag else {
                return None::<()>;
            };
            if !tag.pseudo_element_chain.is_empty() {
                return None::<()>;
            }

            let Some(rect) = fragment_absolute_rect(fragment, containing_block) else {
                return None::<()>;
            };
            let style = terminal_css_properties(&base.style());

            metadata
                .entry(tag.node.id())
                .and_modify(|entry: &mut FragmentMetadata| {
                    entry.rect = entry.rect.union(&rect);
                    entry.fragment_count += 1;
                })
                .or_insert(FragmentMetadata {
                    rect,
                    style,
                    fragment_count: 1,
                });

            None::<()>
        });
    }

    metadata
}

fn fragment_absolute_rect(
    fragment: &Fragment,
    containing_block: &PhysicalRect<Au>,
) -> Option<Rect<f32, CSSPixel>> {
    let rect = match fragment {
        Fragment::Box(box_fragment) | Fragment::Float(box_fragment) => {
            box_fragment.borrow().cumulative_border_box_rect()
        },
        Fragment::Positioning(positioning_fragment) => {
            let positioning_fragment = positioning_fragment.borrow();
            positioning_fragment.offset_by_containing_block(&positioning_fragment.base.rect)
        },
        Fragment::Text(text_fragment) => text_fragment
            .borrow()
            .base
            .rect
            .translate(containing_block.origin.to_vector()),
        Fragment::Image(image_fragment) => image_fragment
            .borrow()
            .base
            .rect
            .translate(containing_block.origin.to_vector()),
        Fragment::IFrame(iframe_fragment) => iframe_fragment
            .borrow()
            .base
            .rect
            .translate(containing_block.origin.to_vector()),
        Fragment::AbsoluteOrFixedPositioned(_) => return None,
    };

    Some(Rect::new(
        Point2D::new(rect.origin.x.to_f32_px(), rect.origin.y.to_f32_px()),
        Size2D::new(rect.size.width.to_f32_px(), rect.size.height.to_f32_px()),
    ))
}

fn terminal_css_properties(style: &ComputedValues) -> TerminalCssProperties {
    let text_color = style.get_inherited_text().clone_color();
    let text_decoration = style.clone_text_decoration_line();
    let font = style.get_font();

    TerminalCssProperties {
        display: terminal_display(Display::from(style.get_box().display)),
        visible: style.get_inherited_box().visibility == Visibility::Visible,
        color: terminal_color(text_color),
        background_color: terminal_color(style.resolve_color(&style.get_background().background_color)),
        bold: font.font_weight.is_bold(),
        italic: font.font_style == FontStyle::ITALIC,
        underline: text_decoration.contains(TextDecorationLine::UNDERLINE),
        overline: text_decoration.contains(TextDecorationLine::OVERLINE),
        line_through: text_decoration.contains(TextDecorationLine::LINE_THROUGH),
        preserve_whitespace: matches!(
            style.clone_white_space_collapse(),
            WhiteSpaceCollapseValue::Preserve |
                WhiteSpaceCollapseValue::PreserveBreaks |
                WhiteSpaceCollapseValue::BreakSpaces
        ),
    }
}

fn terminal_display(display: Display) -> TerminalDisplay {
    match display {
        Display::None => TerminalDisplay::None,
        Display::Contents => TerminalDisplay::Contents,
        Display::GeneratingBox(DisplayGeneratingBox::OutsideInside { outside, inside }) => {
            match (outside, inside) {
                (DisplayOutside::Inline, DisplayInside::Flow { .. }) => TerminalDisplay::Inline,
                (DisplayOutside::Block, DisplayInside::Flow { .. }) => TerminalDisplay::Block,
                (DisplayOutside::Inline, DisplayInside::FlowRoot { .. }) => {
                    TerminalDisplay::InlineBlock
                },
                (DisplayOutside::Block, DisplayInside::FlowRoot { .. }) => {
                    TerminalDisplay::FlowRoot
                },
                (DisplayOutside::Inline, DisplayInside::Flex) => TerminalDisplay::InlineFlex,
                (DisplayOutside::Block, DisplayInside::Flex) => TerminalDisplay::Flex,
                (DisplayOutside::Inline, DisplayInside::Grid) => TerminalDisplay::InlineGrid,
                (DisplayOutside::Block, DisplayInside::Grid) => TerminalDisplay::Grid,
                (DisplayOutside::Inline, DisplayInside::Table) => TerminalDisplay::InlineTable,
                (DisplayOutside::Block, DisplayInside::Table) => TerminalDisplay::Table,
            }
        },
        Display::GeneratingBox(DisplayGeneratingBox::LayoutInternal(internal)) => match internal {
            DisplayLayoutInternal::TableCaption => TerminalDisplay::TableCaption,
            DisplayLayoutInternal::TableCell => TerminalDisplay::TableCell,
            DisplayLayoutInternal::TableColumn => TerminalDisplay::TableColumn,
            DisplayLayoutInternal::TableColumnGroup => TerminalDisplay::TableColumnGroup,
            DisplayLayoutInternal::TableFooterGroup => TerminalDisplay::TableFooterGroup,
            DisplayLayoutInternal::TableHeaderGroup => TerminalDisplay::TableHeaderGroup,
            DisplayLayoutInternal::TableRow => TerminalDisplay::TableRow,
            DisplayLayoutInternal::TableRowGroup => TerminalDisplay::TableRowGroup,
        },
    }
}

fn terminal_color(color: AbsoluteColor) -> TerminalColor {
    let color = color.to_color_space(ColorSpace::Srgb);
    let component = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;

    TerminalColor {
        r: component(color.components.0),
        g: component(color.components.1),
        b: component(color.components.2),
        a: component(color.alpha),
    }
}

fn into_public_layout_node(node: TerminalLayoutNode) -> embedder_traits::TerminalLayoutNode {
    embedder_traits::TerminalLayoutNode {
        kind: into_public_node_kind(node.kind),
        rect: node.rect.map(into_public_rect),
        style: node.style.map(into_public_css_properties),
        children: node
            .children
            .into_iter()
            .map(into_public_layout_node)
            .collect(),
    }
}

fn into_public_rect(rect: Rect<f32, CSSPixel>) -> embedder_traits::TerminalRect {
    embedder_traits::TerminalRect {
        x: rect.origin.x,
        y: rect.origin.y,
        width: rect.size.width,
        height: rect.size.height,
    }
}

fn into_public_node_kind(kind: TerminalNodeKind) -> embedder_traits::TerminalNodeKind {
    match kind {
        TerminalNodeKind::Document => embedder_traits::TerminalNodeKind::Document,
        TerminalNodeKind::Text(text) => embedder_traits::TerminalNodeKind::Text(text),
        TerminalNodeKind::Element(element) => {
            embedder_traits::TerminalNodeKind::Element(embedder_traits::TerminalElementData {
                tag_name: element.tag_name,
                role: into_public_element_role(element.role),
                href: element.href,
                src: element.src,
                alt: element.alt,
                value: element.value,
                title: element.title,
            })
        },
    }
}

fn into_public_element_role(role: TerminalElementRole) -> embedder_traits::TerminalElementRole {
    match role {
        TerminalElementRole::Generic => embedder_traits::TerminalElementRole::Generic,
        TerminalElementRole::Link => embedder_traits::TerminalElementRole::Link,
        TerminalElementRole::Button => embedder_traits::TerminalElementRole::Button,
        TerminalElementRole::Input => embedder_traits::TerminalElementRole::Input,
        TerminalElementRole::TextArea => embedder_traits::TerminalElementRole::TextArea,
        TerminalElementRole::Image => embedder_traits::TerminalElementRole::Image,
        TerminalElementRole::Select => embedder_traits::TerminalElementRole::Select,
        TerminalElementRole::IFrame => embedder_traits::TerminalElementRole::IFrame,
    }
}

fn into_public_css_properties(
    properties: TerminalCssProperties,
) -> embedder_traits::TerminalCssProperties {
    embedder_traits::TerminalCssProperties {
        display: into_public_display(properties.display),
        visible: properties.visible,
        color: into_public_color(properties.color),
        background_color: into_public_color(properties.background_color),
        bold: properties.bold,
        italic: properties.italic,
        underline: properties.underline,
        overline: properties.overline,
        line_through: properties.line_through,
        preserve_whitespace: properties.preserve_whitespace,
    }
}

fn into_public_display(display: TerminalDisplay) -> embedder_traits::TerminalDisplay {
    match display {
        TerminalDisplay::None => embedder_traits::TerminalDisplay::None,
        TerminalDisplay::Contents => embedder_traits::TerminalDisplay::Contents,
        TerminalDisplay::Inline => embedder_traits::TerminalDisplay::Inline,
        TerminalDisplay::Block => embedder_traits::TerminalDisplay::Block,
        TerminalDisplay::InlineBlock => embedder_traits::TerminalDisplay::InlineBlock,
        TerminalDisplay::FlowRoot => embedder_traits::TerminalDisplay::FlowRoot,
        TerminalDisplay::Flex => embedder_traits::TerminalDisplay::Flex,
        TerminalDisplay::InlineFlex => embedder_traits::TerminalDisplay::InlineFlex,
        TerminalDisplay::Grid => embedder_traits::TerminalDisplay::Grid,
        TerminalDisplay::InlineGrid => embedder_traits::TerminalDisplay::InlineGrid,
        TerminalDisplay::Table => embedder_traits::TerminalDisplay::Table,
        TerminalDisplay::InlineTable => embedder_traits::TerminalDisplay::InlineTable,
        TerminalDisplay::TableCaption => embedder_traits::TerminalDisplay::TableCaption,
        TerminalDisplay::TableCell => embedder_traits::TerminalDisplay::TableCell,
        TerminalDisplay::TableRow => embedder_traits::TerminalDisplay::TableRow,
        TerminalDisplay::TableRowGroup => embedder_traits::TerminalDisplay::TableRowGroup,
        TerminalDisplay::TableColumn => embedder_traits::TerminalDisplay::TableColumn,
        TerminalDisplay::TableColumnGroup => embedder_traits::TerminalDisplay::TableColumnGroup,
        TerminalDisplay::TableHeaderGroup => embedder_traits::TerminalDisplay::TableHeaderGroup,
        TerminalDisplay::TableFooterGroup => embedder_traits::TerminalDisplay::TableFooterGroup,
    }
}

fn into_public_color(color: TerminalColor) -> embedder_traits::TerminalColor {
    embedder_traits::TerminalColor {
        r: color.r,
        g: color.g,
        b: color.b,
        a: color.a,
    }
}

#[cfg(test)]
mod tests {
    use app_units::Au;
    use euclid::{Point2D, Rect, Size2D};
    use style::Zero;
    use style::properties::ComputedValues;
    use style::properties::style_structs::Font;

    use super::*;
    use crate::ArcRefCell;
    use crate::fragment_tree::{BaseFragmentInfo, BoxFragment};
    use crate::geom::PhysicalSides;

    fn rect(x: i32, y: i32, width: i32, height: i32) -> PhysicalRect<Au> {
        Rect::new(
            Point2D::new(Au::from_px(x), Au::from_px(y)),
            Size2D::new(Au::from_px(width), Au::from_px(height)),
        )
    }

    fn style() -> ServoArc<ComputedValues> {
        ComputedValues::initial_values_with_font_override(Font::initial_values()).to_arc()
    }

    fn box_fragment(
        id: usize,
        content_rect: PhysicalRect<Au>,
        children: Vec<Fragment>,
    ) -> Fragment {
        Fragment::Box(ArcRefCell::new(BoxFragment::new(
            BaseFragmentInfo::new_for_testing(id),
            style(),
            children,
            content_rect,
            PhysicalSides::zero(),
            PhysicalSides::zero(),
            PhysicalSides::zero(),
            None,
        )))
    }

    #[test]
    fn collects_absolute_rects_for_nested_fragments() {
        let child = box_fragment(2, rect(3, 4, 10, 20), Vec::new());
        let parent = box_fragment(1, rect(5, 7, 100, 80), vec![child]);

        let metadata = collect_fragment_metadata_from_roots(&[parent], rect(0, 0, 800, 600));

        assert_eq!(
            metadata.get(&1).unwrap().rect,
            Rect::new(Point2D::new(5.0, 7.0), Size2D::new(100.0, 80.0))
        );
        assert_eq!(
            metadata.get(&2).unwrap().rect,
            Rect::new(Point2D::new(8.0, 11.0), Size2D::new(10.0, 20.0))
        );
    }

    #[test]
    fn terminal_styles_capture_core_text_and_color_state() {
        let style = style();
        let properties = terminal_css_properties(&style);

        assert_eq!(properties.display, TerminalDisplay::Inline);
        assert!(properties.visible);
        assert!(!properties.bold);
        assert!(!properties.italic);
        assert!(!properties.underline);
        assert_eq!(
            properties.color,
            TerminalColor {
                r: 0,
                g: 0,
                b: 0,
                a: 255,
            }
        );
    }
}
