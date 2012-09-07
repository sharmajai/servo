#[doc="High-level interface to CSS selector matching."]

import std::arc::{ARC, get, clone};

import css::values::{DisplayType, DisplayNone, Inline, Block, Unit, Auto};
import css::values::Stylesheet;
import dom::base::{HTMLDivElement, HTMLHeadElement, HTMLImageElement, UnknownElement, HTMLScriptElement};
import dom::base::{Comment, Doctype, Element, Node, NodeKind, Text};
import util::color::{Color, rgb};
import util::color::css_colors::{white, black};
import layout::base::{LayoutData, NTree};

type SpecifiedStyle = {mut background_color : Option<Color>,
                        mut display_type : Option<DisplayType>,
                        mut font_size : Option<Unit>,
                        mut height : Option<Unit>,
                        mut text_color : Option<Color>,
                        mut width : Option<Unit>
                       };

trait DefaultStyleMethods {
    fn default_color() -> Color;
    fn default_display_type() -> DisplayType;
    fn default_width() -> Unit;
    fn default_height() -> Unit;
}

/// Default styles for various attributes in case they don't get initialized from CSS selectors.
impl NodeKind : DefaultStyleMethods {
    fn default_color() -> Color {
        match self {
          Text(*) => white(),
          Element(*) => white(),
            _ => fail ~"unstyleable node type encountered"
        }
    }

    fn default_display_type() -> DisplayType {
        match self {
          Text(*) => { Inline }
          Element(element) => {
            match *element.kind {
              HTMLDivElement => Block,
              HTMLHeadElement => DisplayNone,
              HTMLImageElement(*) => Inline,
              HTMLScriptElement => DisplayNone,
              UnknownElement => Inline,
            }
          },
          Comment(*) | Doctype(*) => DisplayNone
        }
    }
    
    fn default_width() -> Unit {
        Auto
    }

    fn default_height() -> Unit {
        Auto
    }
}

/**
 * Create a specified style that can be used to initialize a node before selector matching.
 *
 * Everything is initialized to none except the display style. The default value of the display
 * style is computed so that it can be used to short-circuit selector matching to avoid computing
 * style for children of display:none objects.
 */
fn empty_style_for_node_kind(kind: NodeKind) -> SpecifiedStyle {
    let display_type = kind.default_display_type();

    {mut background_color : None,
     mut display_type : Some(display_type),
     mut font_size : None,
     mut height : None,
     mut text_color : None,
     mut width : None}
}

trait StylePriv {
    fn initialize_style() -> ~[@LayoutData];
}

impl Node : StylePriv {
    #[doc="
        Set a default auxiliary data so that other threads can modify it.
        
        This is, importantly, the function that creates the layout
        data for the node (the reader-auxiliary box in the RCU model)
        and populates it with the default style.

     "]
    // TODO: we should look into folding this into building the dom,
    // instead of doing a linear sweep afterwards.
    fn initialize_style() -> ~[@LayoutData] {
        if !self.has_aux() {
            let node_kind = self.read(|n| copy *n.kind);
            let the_layout_data = @LayoutData({
                mut specified_style : ~empty_style_for_node_kind(node_kind),
                mut box : None
            });

            self.set_aux(the_layout_data);

            ~[the_layout_data]
        } else {
            ~[]
        }
    }
}

trait StyleMethods {
    fn initialize_style_for_subtree() -> ~[@LayoutData];
    fn get_specified_style() -> SpecifiedStyle;
    fn recompute_style_for_subtree(styles : ARC<Stylesheet>);
}

impl Node : StyleMethods {
    #[doc="Sequentially initialize the nodes' auxilliary data so they can be updated in parallel."]
    fn initialize_style_for_subtree() -> ~[@LayoutData] {
        let mut handles = self.initialize_style();
        
        for NTree.each_child(self) |kid| {
            handles += kid.initialize_style_for_subtree();
        }

        return handles;
    }
    
    #[doc="
        Returns the computed style for the given node. If CSS selector matching has not yet been
        performed, fails.

        TODO: Return a safe reference; don't copy.
    "]
    fn get_specified_style() -> SpecifiedStyle {
        if !self.has_aux() {
            fail ~"get_specified_style() called on a node without a style!";
        }
        return copy *self.aux(|x| copy x).specified_style;
    }

    #[doc="
        Performs CSS selector matching on a subtree.

        This is, importantly, the function that updates the layout data for the node (the reader-
        auxiliary box in the RCU model) with the computed style.
    "]
    fn recompute_style_for_subtree(styles : ARC<Stylesheet>) {
        listen(|ack_chan| {
            let mut i = 0u;
            
            // Compute the styles of each of our children in parallel
            for NTree.each_child(self) |kid| {
                i = i + 1u;
                let new_styles = clone(&styles);
                
                task::spawn(|| {
                    kid.recompute_style_for_subtree(new_styles); 
                    ack_chan.send(());
                })
            }

            self.match_css_style(*get(&styles));
            
            // Make sure we have finished updating the tree before returning
            while i > 0 {
                ack_chan.recv();
                i = i - 1u;
            }
        })
    }
}