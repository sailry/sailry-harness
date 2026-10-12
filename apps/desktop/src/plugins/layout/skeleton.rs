use gpui_shell::{HostModule, HostValue};

pub(super) fn extend(module: HostModule) -> HostModule {
    let declarations = format!(
        "{}\n{}",
        module.declared().unwrap_or_default(),
        r#"
            /** Kit Skeleton rows using the shared card geometry. Keep loaded rows during refresh. */
            export const SkeletonList: { new(id: string, props?: { rows?: number }): import("gpui-kit").Element };
        "#
    );
    module
        .component("SkeletonList", |args, _, cx| {
            let rows = args
                .props()
                .get("rows")
                .and_then(HostValue::as_number)
                .filter(|rows| rows.is_finite())
                .unwrap_or(3.)
                .clamp(1., 20.) as usize;
            crate::ui::skeleton::list(args.id().to_owned(), rows, cx)
        })
        .declarations(declarations)
}
