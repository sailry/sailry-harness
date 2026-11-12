import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/notices.dart';
import '../../../runtime/session.dart' show HostConnection;
import '../../../ui/kit.dart';
import 'controller.dart';
import 'create_sheet.dart';
import 'field_sheet.dart';
import 'parts.dart';
import 'provider_page.dart';
import 'rules.dart';

/// Model services of one Node, including Desktop presets and logins.
class ProvidersPage extends StatefulWidget {
  const ProvidersPage({super.key, required this.host});
  final HostConnection host;

  @override
  State<ProvidersPage> createState() => _ProvidersPageState();
}

class _ProvidersPageState extends State<ProvidersPage> {
  List<Json>? _providers;
  Object? _error;
  bool _loading = false;

  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    setState(() => _loading = true);
    try {
      final providers = await ProviderController.list(widget.host);
      if (mounted) {
        setState(() {
          _providers = providers;
          _error = null;
        });
      }
    } catch (error) {
      if (!mounted) return;
      if (_providers == null) {
        setState(() => _error = error);
      } else {
        showFailure(context, error);
      }
    } finally {
      if (mounted) setState(() => _loading = false);
    }
  }

  Set<String> _names([String? except]) => {
    for (final provider in _providers ?? const <Json>[])
      if (provider['id'] != except) text(provider['name']),
  };

  Future<void> _open(Json provider) async {
    await pushPage(
      context,
      ProviderPage(
        controller: ProviderController(widget.host, provider),
        names: _names(text(provider['id'])),
      ),
    );
    if (mounted) await _load();
  }

  Future<void> _create() async {
    final provider = await createProvider(
      context,
      host: widget.host,
      names: _names(),
    );
    if (provider != null && mounted) await _open(provider);
  }

  @override
  Widget build(BuildContext context) {
    final providers = _providers;
    return PageFrame(
      title: context.tr('providers'),
      loading: _loading,
      actions: [
        RoundButton(
          icon: 'plus',
          tooltip: context.tr('providerNew'),
          onPressed: providers == null ? null : _create,
        ),
      ],
      failure: providers == null && _error != null
          ? FailureState(
              icon: 'spark',
              message: failureText(_error!, translate: context.tr),
              onRetry: _load,
            )
          : null,
      empty: providers?.isEmpty == true
          ? EmptyState(icon: 'spark', message: context.tr('providerEmpty'))
          : null,
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          if (providers != null && providers.isNotEmpty)
            Surface(
              padding: const EdgeInsets.symmetric(horizontal: 16, vertical: 4),
              child: Column(
                children: [
                  for (final (index, provider) in providers.indexed)
                    _ProviderRow(
                      provider: provider,
                      divider: index < providers.length - 1,
                      onTap: () => _open(provider),
                    ),
                ],
              ),
            ),
          if (providers != null) ...[
            const SizedBox(height: 12),
            Padding(
              padding: const EdgeInsets.symmetric(horizontal: 8),
              child: Text(
                context.tr('providerDesktopHint'),
                textAlign: TextAlign.center,
                style: dimText(context, 12),
              ),
            ),
          ],
        ],
      ),
    );
  }
}

class _ProviderRow extends StatelessWidget {
  const _ProviderRow({
    required this.provider,
    required this.divider,
    required this.onTap,
  });
  final Json provider;
  final bool divider;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    final colors = Theme.of(context).colorScheme;
    final name = text(provider['name']);
    final count = models(provider).length;
    final detail = [
      endpointHost(text(provider['endpoint'])),
      apiLabel(context, text(provider['api'])),
    ].where((part) => part.isNotEmpty).join(' · ');
    return Opacity(
      opacity: provider['enabled'] == false ? .45 : 1,
      child: InkWell(
        onTap: onTap,
        child: DecoratedBox(
          decoration: BoxDecoration(
            border: divider
                ? Border(bottom: BorderSide(color: colors.outlineVariant))
                : null,
          ),
          child: Padding(
            padding: const EdgeInsets.symmetric(vertical: 11),
            child: Row(
              children: [
                ProviderAvatar(name),
                const SizedBox(width: 12),
                Expanded(
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.start,
                    children: [
                      Text(
                        name,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: const TextStyle(
                          fontSize: 15,
                          fontWeight: FontWeight.w500,
                        ),
                      ),
                      const SizedBox(height: 2),
                      Text(
                        detail,
                        maxLines: 1,
                        overflow: TextOverflow.ellipsis,
                        style: dimText(context, 12),
                      ),
                    ],
                  ),
                ),
                const SizedBox(width: 8),
                Text(
                  context
                      .tr('providerModelCount')
                      .replaceAll('{count}', '$count'),
                  style: TextStyle(
                    fontSize: 13,
                    color: colors.onSurfaceVariant,
                  ),
                ),
                const SizedBox(width: 8),
                AppIcon('chevron', color: colors.onSurfaceVariant),
              ],
            ),
          ),
        ),
      ),
    );
  }
}
