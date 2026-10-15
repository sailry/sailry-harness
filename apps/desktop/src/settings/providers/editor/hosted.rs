use super::super::vendors::{Region, Vendor};
use super::*;
use gpui_kit::component::{
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
};
use sailry_protocol::conversation::connection::Product;

#[derive(Clone)]
pub(super) struct Offering {
    product: Product,
    vendor: Vendor,
}

impl SearchableListItem for Offering {
    type Value = Product;

    fn title(&self) -> SharedString {
        tr(match self.product {
            Product::Metered => "provider_metered",
            Product::Coding => self.vendor.coding_key(),
        })
    }

    fn value(&self) -> &Self::Value {
        &self.product
    }
}

pub(super) fn new(
    preset: Preset,
    product: Option<Product>,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Entity<SelectState<Vec<Offering>>> {
    let (items, selected) = values(preset, product);
    let state = cx.new(|cx| SelectState::new(items, selected, window, cx));
    cx.subscribe_in(&state, window, |editor, _, event, window, cx| {
        if let SelectEvent::Confirm(Some(product)) = event {
            editor.select_product(*product, window, cx);
        }
    })
    .detach();
    state
}

fn values(preset: Preset, product: Option<Product>) -> (Vec<Offering>, Option<IndexPath>) {
    let Preset::Hosted(vendor) = preset else {
        return (Vec::new(), None);
    };
    (
        [Product::Metered, Product::Coding]
            .into_iter()
            .map(|product| Offering { product, vendor })
            .collect(),
        product.map(|product| IndexPath::new(usize::from(product == Product::Coding))),
    )
}

pub(super) fn routes(vendor: Vendor, product: Product) -> &'static [(Region, &'static str)] {
    match product {
        Product::Metered => vendor.endpoints(),
        Product::Coding => vendor.coding_endpoints(),
    }
}

impl Editor {
    pub(super) fn product(&self) -> Product {
        self.hosted_product.unwrap_or(Product::Metered)
    }

    pub(super) fn sync_product(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (items, selected) = values(self.preset, self.hosted_product);
        self.offering.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            state.set_selected_index(selected, window, cx);
        });
    }

    pub(super) fn select_product(
        &mut self,
        product: Product,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.closed || self.hosted_product == Some(product) {
            return;
        }
        let Preset::Hosted(vendor) = self.preset else {
            return;
        };
        if vendor.coding_endpoints().is_empty() {
            return;
        }
        let region = vendor.region(self.endpoint.read(cx).value().as_ref());
        let endpoints = routes(vendor, product);
        let endpoint = endpoints
            .iter()
            .find(|(value, _)| Some(*value) == region)
            .or_else(|| endpoints.first())
            .map_or(vendor.endpoint(), |(_, url)| *url);
        self.hosted_product = Some(product);
        self.endpoint
            .update(cx, |input, cx| input.set_value(endpoint, window, cx));
        self.clear_route(window, cx);
        self.sync_product(window, cx);
        self.sync_region(window, cx);
        self.seed_coding(window, cx);
        cx.notify();
    }

    pub(super) fn clear_route(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.probe = None;
        self.models.clear();
        self.default_model = 0;
        self.configured = false;
        self.original_key = None;
        self.key_ready = true;
        self.credential
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.error = None;
    }

    pub(super) fn seed_coding(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Preset::Hosted(vendor) = self.preset else {
            return;
        };
        if self.product() != Product::Coding || !self.models.is_empty() {
            return;
        }
        let Some(id) = vendor.coding_model() else {
            return;
        };
        self.models.push(Draft::discovered(
            self.next_model,
            sailry_protocol::conversation::discovery::Model {
                id: id.into(),
                display_name: None,
                context: None,
                output: None,
                capabilities: None,
            },
            None,
            self.preset,
            window,
            cx,
        ));
        self.next_model += 1;
    }

    pub(super) fn metadata_only(&self) -> bool {
        self.preset.cloud()
            || (self.product() == Product::Coding
                && matches!(self.preset, Preset::Hosted(vendor) if vendor != Vendor::MiniMax))
    }

    pub(super) fn product_form(&self, form: Form) -> Form {
        if !matches!(self.preset, Preset::Hosted(vendor) if !vendor.coding_endpoints().is_empty()) {
            return form;
        }
        form.child(
            Field::new().label(tr("provider_product")).child(
                div()
                    .w_full()
                    .debug_selector(|| "provider-product".into())
                    .child(
                        Select::new(&self.offering)
                            .id("provider-product-select")
                            .w_full()
                            .disabled(self.pending)
                            .placeholder(tr("provider_product"))
                            .accessibility_label(tr("provider_product")),
                    ),
            ),
        )
    }
}
