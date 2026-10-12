use super::super::vendors::Region;
use super::*;
use gpui_kit::component::{
    searchable_list::SearchableListItem,
    select::{Select, SelectEvent, SelectState},
};
use sailry_protocol::conversation::connection::Product;

impl SearchableListItem for Region {
    type Value = Self;

    fn title(&self) -> SharedString {
        tr(self.key())
    }

    fn value(&self) -> &Self::Value {
        self
    }
}

pub(super) fn new(
    preset: Preset,
    endpoint: &str,
    product: Product,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> Entity<SelectState<Vec<Region>>> {
    let (items, selected) = values(preset, endpoint, product);
    let state = cx.new(|cx| SelectState::new(items, selected, window, cx));
    cx.subscribe_in(&state, window, |editor, _, event, window, cx| {
        if let SelectEvent::Confirm(Some(region)) = event {
            editor.select_region(*region, window, cx);
        }
    })
    .detach();
    state
}

fn values(preset: Preset, endpoint: &str, product: Product) -> (Vec<Region>, Option<IndexPath>) {
    let Preset::Hosted(vendor) = preset else {
        return (Vec::new(), None);
    };
    let items: Vec<_> = hosted::routes(vendor, product)
        .iter()
        .map(|(region, _)| *region)
        .collect();
    let selected = vendor
        .region(endpoint)
        .and_then(|region| items.iter().position(|item| *item == region))
        .map(IndexPath::new);
    (items, selected)
}

impl Editor {
    pub(super) fn sync_region(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (items, selected) = values(
            self.preset,
            self.endpoint.read(cx).value().as_ref(),
            self.product(),
        );
        self.region.update(cx, |state, cx| {
            state.set_items(items, window, cx);
            state.set_selected_index(selected, window, cx);
        });
    }

    pub(super) fn select_region(
        &mut self,
        region: Region,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.pending || self.closed {
            return;
        }
        let Preset::Hosted(vendor) = self.preset else {
            return;
        };
        if self.hosted_product.is_none() && !vendor.coding_endpoints().is_empty() {
            return;
        }
        let Some((_, endpoint)) = hosted::routes(vendor, self.product())
            .iter()
            .find(|(value, _)| *value == region)
        else {
            return;
        };
        if vendor.region(self.endpoint.read(cx).value().as_ref()) == Some(region) {
            return;
        }
        self.endpoint
            .update(cx, |input, cx| input.set_value(*endpoint, window, cx));
        self.clear_route(window, cx);
        self.sync_region(window, cx);
        self.seed_coding(window, cx);
        cx.notify();
    }

    pub(super) fn region_form(&self, form: Form) -> Form {
        if !matches!(self.preset, Preset::Hosted(vendor) if vendor.endpoints().len() > 1) {
            return form;
        }
        form.child(
            Field::new().label(tr("provider_region")).child(
                div()
                    .w_full()
                    .debug_selector(|| "provider-region".into())
                    .child(
                        Select::new(&self.region)
                            .id("provider-region-select")
                            .w_full()
                            .disabled(self.pending || self.hosted_product.is_none())
                            .placeholder(tr("provider_region"))
                            .accessibility_label(tr("provider_region")),
                    ),
            ),
        )
    }
}
