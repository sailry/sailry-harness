import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../ui/kit.dart';
import '../../../ui/theme.dart';
import 'connection_card.dart';
import 'controller.dart';
import 'discovery_sheet.dart';
import 'field_sheet.dart';
import 'model_page.dart';
import 'models_card.dart';
import 'parts.dart';
import 'rules.dart';

/// Connection and models of one provider.
class ProviderPage extends StatefulWidget {
  const ProviderPage({
    super.key,
    required this.controller,
    required this.names,
  });
  final ProviderController controller;

  /// Names of the other providers, which a rename must not reuse.
  final Set<String> names;

  @override
  State<ProviderPage> createState() => _ProviderPageState();
}

class _ProviderPageState extends State<ProviderPage> {
  ProviderController get _controller => widget.controller;

  @override
  void dispose() {
    _controller.dispose();
    super.dispose();
  }

  Future<void> _fetch() => showDiscoverySheet(context, _controller);

  Future<void> _add() => editText(
    context,
    title: context.tr('providerModelId'),
    check: (id) => findModel(_controller.provider, id) == null
        ? null
        : 'providerModelExists',
    onSave: (id) async {
      final references = await _controller.references([id]);
      if (!mounted) return false;
      return _controller.save(
        mergeDiscovered(_controller.provider, [
          {'id': id},
        ], references),
      );
    },
  );

  Future<void> _remove() async {
    final name = text(_controller.provider['name']);
    if (!await confirmDelete(context, name) || !mounted) return;
    if (await attempt(context, _controller.remove) && mounted) {
      Navigator.pop(context);
    }
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _controller,
    builder: (context, _) {
      final provider = _controller.provider;
      final idle = !_controller.busy;
      return PageFrame(
        title: text(provider['name']),
        loading: _controller.busy,
        actions: [_MoreMenu(onDelete: idle ? _remove : null)],
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            ConnectionCard(controller: _controller, names: widget.names),
            const SizedBox(height: 16),
            ModelsCard(
              provider: provider,
              onFetch: idle ? _fetch : null,
              onAdd: idle ? _add : null,
              onOpen: (model) => pushPage(
                context,
                ModelPage(controller: _controller, id: text(model['id'])),
              ),
            ),
          ],
        ),
      );
    },
  );
}

class _MoreMenu extends StatelessWidget {
  const _MoreMenu({required this.onDelete});
  final VoidCallback? onDelete;

  @override
  Widget build(BuildContext context) => MenuAnchor(
    style: const MenuStyle(
      backgroundColor: WidgetStatePropertyAll(Colors.transparent),
      surfaceTintColor: WidgetStatePropertyAll(Colors.transparent),
      elevation: WidgetStatePropertyAll(0),
      padding: WidgetStatePropertyAll(EdgeInsets.zero),
    ),
    alignmentOffset: const Offset(-140, 4),
    menuChildren: [
      Surface(
        kind: SurfaceKind.sheet,
        radius: 16,
        padding: const EdgeInsets.all(6),
        child: MenuItemButton(
          onPressed: onDelete,
          leadingIcon: AppIcon(
            'trash',
            size: 16,
            color: SailryTheme.statusColor(context, StatusTone.danger),
          ),
          child: Text(
            context.tr('providerDelete'),
            style: TextStyle(
              color: SailryTheme.statusColor(context, StatusTone.danger),
            ),
          ),
        ),
      ),
    ],
    builder: (context, controller, _) => RoundButton(
      icon: 'more',
      tooltip: context.tr('more'),
      onPressed: () =>
          controller.isOpen ? controller.close() : controller.open(),
    ),
  );
}
