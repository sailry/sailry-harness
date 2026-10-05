import 'package:flutter/cupertino.dart';
import 'package:flutter/material.dart';
import '../l10n/strings.dart';
import 'icons.dart';
import 'theme.dart';
import 'surface.dart';
import 'app_background.dart';
import 'failure_state.dart';
import 'loading.dart';
import 'page_heading.dart';

export 'loading.dart' show LoadingOverlay;

export 'icons.dart' show AppIcon, DisclosureIcon;
export 'empty_state.dart' show EmptyState;
export 'failure_state.dart' show FailureState;
export 'form.dart' show FormBody, SelectField;
export 'host_state.dart' show HostState;
export 'surface.dart' show Surface;

class RoundButton extends StatelessWidget {
  const RoundButton({
    super.key,
    required this.icon,
    required this.onPressed,
    this.tooltip,
    this.primary = false,
  });
  final String icon;
  final VoidCallback? onPressed;
  final String? tooltip;
  final bool primary;
  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    return SizedBox.square(
      dimension: 36,
      child: IconButton.filledTonal(
        padding: EdgeInsets.zero,
        tooltip: tooltip,
        style: IconButton.styleFrom(
          backgroundColor: primary ? colors.primary : colors.surfaceContainer,
          foregroundColor: primary ? colors.onPrimary : colors.onSurfaceVariant,
          disabledForegroundColor: Theme.of(context).disabledColor,
        ),
        onPressed: onPressed,
        icon: AppIcon(icon, size: 18),
      ),
    );
  }
}

class SelectorCard extends StatelessWidget {
  const SelectorCard({
    super.key,
    required this.title,
    this.subtitle,
    this.icon = 'folder',
    this.leading,
    required this.onTap,
  });
  final String title;
  final String? subtitle;
  final String icon;
  final Widget? leading;
  final VoidCallback? onTap;
  @override
  Widget build(BuildContext context) => Surface(
    onTap: onTap,
    padding: const EdgeInsets.all(14),
    child: Row(
      children: [
        Container(
          width: 38,
          height: 38,
          decoration: BoxDecoration(
            color: Theme.of(context).colorScheme.surfaceContainerHigh,
            borderRadius: BorderRadius.circular(12),
          ),
          child: Center(child: leading ?? AppIcon(icon)),
        ),
        const SizedBox(width: 11),
        Expanded(
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                title,
                style: Theme.of(context).textTheme.titleMedium,
                overflow: TextOverflow.ellipsis,
              ),
              if (subtitle != null) ...[
                const SizedBox(height: 2),
                Row(
                  children: [
                    const AppIcon('branch', size: 14),
                    const SizedBox(width: 5),
                    Flexible(
                      child: Text(
                        subtitle!,
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.onSurfaceVariant,
                        ),
                        overflow: TextOverflow.ellipsis,
                      ),
                    ),
                  ],
                ),
              ],
            ],
          ),
        ),
        if (onTap != null) ...[
          const SizedBox(width: 8),
          const AppIcon('down', size: 14),
        ],
      ],
    ),
  );
}

class PageFrame extends StatelessWidget {
  const PageFrame({
    super.key,
    required this.title,
    required this.child,
    this.actions = const [],
    this.scroll = true,
    this.loading = false,
    this.padding = const EdgeInsets.symmetric(horizontal: 20),
    this.bottom,
    this.empty,
    this.failure,
    this.titleSize = 22,
    this.backEnabled = true,
  });
  final String title;
  final List<Widget> actions;
  final Widget child;
  final bool scroll;
  final bool loading;
  final EdgeInsetsGeometry padding;
  final Widget? bottom;
  final Widget? empty;
  final Widget? failure;
  final double titleSize;
  final bool backEnabled;
  @override
  Widget build(BuildContext context) => Scaffold(
    backgroundColor: Colors.transparent,
    body: SafeArea(
      bottom: false,
      child: Column(
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(20, 19, 20, 16),
            child: Row(
              children: [
                Expanded(
                  child: PageHeading(
                    title: title,
                    size: titleSize,
                    enabled: backEnabled,
                  ),
                ),
                if (actions.isNotEmpty) const SizedBox(width: 8),
                ...actions.expand((item) => [const SizedBox(width: 4), item]),
              ],
            ),
          ),
          Expanded(
            child: LoadingOverlay(
              loading: loading,
              child: failure != null
                  ? Padding(
                      padding: padding.add(
                        EdgeInsets.only(
                          bottom: 24 + MediaQuery.paddingOf(context).bottom,
                        ),
                      ),
                      child: failure,
                    )
                  : empty != null
                  ? CustomScrollView(
                      keyboardDismissBehavior:
                          ScrollViewKeyboardDismissBehavior.onDrag,
                      slivers: [
                        SliverPadding(
                          padding: padding,
                          sliver: SliverToBoxAdapter(child: child),
                        ),
                        SliverFillRemaining(
                          hasScrollBody: false,
                          child: Padding(
                            padding: padding.add(
                              EdgeInsets.only(
                                bottom:
                                    24 + MediaQuery.paddingOf(context).bottom,
                              ),
                            ),
                            child: empty,
                          ),
                        ),
                      ],
                    )
                  : scroll
                  ? SingleChildScrollView(
                      keyboardDismissBehavior:
                          ScrollViewKeyboardDismissBehavior.onDrag,
                      padding: padding.add(
                        EdgeInsets.only(
                          bottom: 24 + MediaQuery.paddingOf(context).bottom,
                        ),
                      ),
                      child: child,
                    )
                  : Padding(padding: padding, child: child),
            ),
          ),
          ?bottom,
        ],
      ),
    ),
  );
}

/// Keep native dialog layout, focus, and dismissal while diffusing the page
/// behind its translucent material. DialogTheme alone cannot add backdrop blur.
Future<T?> showAppDialog<T>({
  required BuildContext context,
  required WidgetBuilder builder,
}) => showDialog<T>(
  context: context,
  barrierColor: const Color(0x52000000),
  builder: (context) => BackdropFilter(
    filter: SailryTheme.glassFilter(context, SurfaceKind.sheet),
    child: builder(context),
  ),
);

Future<T?> showAppSheet<T>(
  BuildContext context,
  String title, {
  required Widget child,
  List<Widget> actions = const [],
  bool scroll = true,
}) => showModalBottomSheet<T>(
  context: context,
  isScrollControlled: true,
  useSafeArea: true,
  isDismissible: true,
  enableDrag: true,
  constraints: const BoxConstraints(maxWidth: 520),
  barrierColor: const Color(0x52000000),
  builder: (sheetContext) => BackdropFilter(
    filter: SailryTheme.sheetBackdrop,
    child: Padding(
      padding: EdgeInsets.fromLTRB(
        12,
        0,
        12,
        12 +
            MediaQuery.paddingOf(sheetContext).bottom +
            MediaQuery.viewInsetsOf(sheetContext).bottom,
      ),
      child: LayoutBuilder(
        builder: (sheetContext, constraints) => Surface(
          kind: SurfaceKind.sheet,
          radius: 28,
          padding: EdgeInsets.zero,
          child: ListTileTheme.merge(
            dense: true,
            minTileHeight: 48,
            minVerticalPadding: 8,
            minLeadingWidth: 20,
            horizontalTitleGap: 10,
            contentPadding: const EdgeInsets.symmetric(horizontal: 8),
            child: ConstrainedBox(
              constraints: BoxConstraints(
                maxHeight: constraints.maxHeight * .78,
              ),
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  Padding(
                    padding: const EdgeInsets.symmetric(vertical: 10),
                    child: Center(
                      child: Container(
                        width: 32,
                        height: 4,
                        decoration: BoxDecoration(
                          borderRadius: BorderRadius.circular(2),
                          color: Theme.of(
                            sheetContext,
                          ).colorScheme.onSurfaceVariant.withValues(alpha: .35),
                        ),
                      ),
                    ),
                  ),
                  if (actions.isNotEmpty)
                    Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 20),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        crossAxisAlignment: CrossAxisAlignment.stretch,
                        children: [...actions, const Divider(height: 12)],
                      ),
                    ),
                  Flexible(
                    child: LayoutBuilder(
                      builder: (context, constraints) {
                        final content = FailureViewport(
                          height: (constraints.maxHeight - 24).clamp(
                            0,
                            double.infinity,
                          ),
                          child: Semantics(
                            namesRoute: true,
                            label: title,
                            child: SizedBox(
                              width: double.infinity,
                              child: child,
                            ),
                          ),
                        );
                        const padding = EdgeInsets.fromLTRB(20, 8, 20, 16);
                        return scroll
                            ? SingleChildScrollView(
                                padding: padding,
                                child: content,
                              )
                            : Padding(padding: padding, child: content);
                      },
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    ),
  ),
);

Future<T?> pushPage<T>(BuildContext context, Widget page) => Navigator.of(
  context,
).push<T>(CupertinoPageRoute<T>(builder: (_) => AppBackground(child: page)));

Future<void> showHostPicker(
  BuildContext context, {
  required String selected,
  bool allowAll = false,
  List<String> additionalHosts = const [],
  VoidCallback? onAdd,
  required ValueChanged<String> onSelected,
}) async {
  final value = await showAppSheet<String>(
    context,
    context.tr('selectHost'),
    actions: onAdd == null
        ? []
        : [
            ListTile(
              leading: const AppIcon('plus'),
              title: Text(context.tr('pair')),
              onTap: () {
                Navigator.of(context).pop();
                onAdd();
              },
            ),
          ],
    child: Builder(
      builder: (sheetContext) => Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          if (allowAll)
            ListTile(
              title: Text(context.tr('allHosts')),
              trailing: selected == 'all' ? const AppIcon('check') : null,
              onTap: () => Navigator.pop(sheetContext, 'all'),
            ),
          for (final host in [
            'Studio',
            'Build Server',
            'MacBook Air',
            ...additionalHosts,
          ])
            ListTile(
              leading: Container(
                width: 7,
                height: 7,
                decoration: BoxDecoration(
                  shape: BoxShape.circle,
                  color: host == 'MacBook Air'
                      ? Theme.of(context).colorScheme.onSurfaceVariant
                      : Theme.of(context).colorScheme.tertiary,
                ),
              ),
              title: Text(host),
              subtitle: Text(
                context.tr(host == 'MacBook Air' ? 'offline' : 'online'),
              ),
              trailing: selected == host ? const AppIcon('check') : null,
              onTap: () => Navigator.pop(sheetContext, host),
            ),
        ],
      ),
    ),
  );
  if (value != null) onSelected(value);
}

class FloatingNavigation extends StatelessWidget {
  const FloatingNavigation({
    super.key,
    required this.selected,
    required this.onSelected,
  });
  final int selected;
  final ValueChanged<int> onSelected;
  @override
  Widget build(BuildContext context) {
    const icons = ['chat', 'server', 'folder', 'settings'];
    const labels = ['chat', 'hosts', 'resources', 'settings'];
    final colors = Theme.of(context).colorScheme;
    return SafeArea(
      top: false,
      minimum: const EdgeInsets.only(bottom: 23),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 18),
        child: Surface(
          kind: SurfaceKind.navigation,
          radius: 32,
          padding: const EdgeInsets.all(5),
          child: ConstrainedBox(
            constraints: const BoxConstraints(minHeight: 54),
            child: Row(
              spacing: 3,
              children: List.generate(
                labels.length,
                (index) => Expanded(
                  child: Semantics(
                    selected: selected == index,
                    child: TextButton(
                      key: ValueKey('tab-$index'),
                      style: TextButton.styleFrom(
                        padding: const EdgeInsets.symmetric(
                          horizontal: 2,
                          vertical: 5,
                        ),
                        visualDensity: VisualDensity.standard,
                        alignment: Alignment.center,
                        minimumSize: const Size(0, 52),
                        tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                        backgroundColor: selected == index
                            ? SailryTheme.navigationSelection(context)
                            : Colors.transparent,
                        shape: RoundedRectangleBorder(
                          borderRadius: BorderRadius.horizontal(
                            left: Radius.circular(index == 0 ? 26 : 18),
                            right: Radius.circular(
                              index == labels.length - 1 ? 26 : 18,
                            ),
                          ),
                        ),
                      ),
                      onPressed: () => onSelected(index),
                      child: Column(
                        mainAxisSize: MainAxisSize.min,
                        children: [
                          AppIcon(
                            icons[index],
                            color: selected == index
                                ? colors.onSurface
                                : colors.onSurfaceVariant,
                          ),
                          const SizedBox(height: 4),
                          Text(
                            context.tr(labels[index]),
                            textAlign: TextAlign.center,
                            style: TextStyle(
                              fontSize: 14,
                              height: 1.2,
                              color: selected == index
                                  ? colors.onSurface
                                  : colors.onSurfaceVariant,
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    );
  }
}
