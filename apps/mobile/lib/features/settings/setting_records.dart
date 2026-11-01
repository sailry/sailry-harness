import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart' show HostConnection;
import '../../ui/kit.dart';
import 'live.dart';
import 'record_form.dart';

Future<void> showSettingRecords(
  BuildContext context, {
  required HostConnection host,
}) async {
  final key = GlobalKey<_RecordsState>();
  await showAppSheet(
    context,
    context.tr('providers'),
    scroll: false,
    actions: [
      ListTile(
        leading: const AppIcon('plus'),
        title: Text(context.tr('add')),
        onTap: () => key.currentState?.edit(),
      ),
    ],
    child: _Records(key: key, host: host),
  );
}

class _Records extends StatefulWidget {
  const _Records({super.key, required this.host});
  final HostConnection host;
  @override
  State<_Records> createState() => _RecordsState();
}

class _RecordsState extends State<_Records> {
  List<Map<String, dynamic>>? _records;
  String? _error;
  bool _pending = false;
  @override
  void initState() {
    super.initState();
    _load();
  }

  Future<void> _load() async {
    try {
      final response = await widget.host.command('list_providers');
      if (mounted) {
        setState(() {
          _records = objects(response['data']);
          _error = null;
        });
      }
    } catch (error) {
      if (mounted) {
        setState(() => _error = failure(error, translate: context.tr));
      }
    }
  }

  Future<void> edit([Map<String, dynamic>? record]) async {
    final saved = await showAppSheet<bool>(
      context,
      context.tr(record == null ? 'add' : 'edit'),
      child: RecordForm(
        host: widget.host,
        record: record,
        names: {
          for (final item in _records ?? <Map<String, dynamic>>[])
            if (item['id'] != record?['id']) string(item['name']),
        },
      ),
    );
    if (saved == true && mounted) await _load();
  }

  Future<void> _remove(Map<String, dynamic> record) async {
    if (_pending) return;
    final name = string(record['name']);
    if (!await confirmRemoval(context, name) || !mounted) return;
    setState(() {
      _pending = true;
      _error = null;
    });
    try {
      await widget.host.command('remove_provider', {
        'provider': record['id'],
        'expected_revision': record['revision'],
      });
      if (mounted) await _load();
    } catch (error) {
      if (mounted) {
        setState(
          () => _error = failure(error, saving: true, translate: context.tr),
        );
      }
    } finally {
      if (mounted) setState(() => _pending = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    if (_records == null && _error != null) {
      return FailureState(icon: 'settings', message: _error!, onRetry: _load);
    }
    final colors = Theme.of(context).colorScheme;
    return LoadingOverlay(
      scroll: true,
      loading: _pending || _records == null && _error == null,
      minHeight: 128,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          settingsError(_error),
          if (_error != null)
            TextButton(
              onPressed: _load,
              child: Text(context.tr('settingsRetry')),
            ),
          if (_records?.isEmpty == true)
            EmptyState(icon: 'spark', message: context.tr('settingsEmpty')),
          SlidableAutoCloseBehavior(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final record in _records ?? <Map<String, dynamic>>[])
                  Padding(
                    padding: const EdgeInsets.only(bottom: 8),
                    child: ClipRRect(
                      borderRadius: BorderRadius.circular(14),
                      child: Slidable(
                        key: ValueKey('setting-providers-${record['id']}'),
                        groupTag: 'providers',
                        endActionPane: ActionPane(
                          motion: const DrawerMotion(),
                          extentRatio: .46,
                          children: [
                            SlidableAction(
                              onPressed: _pending ? null : (_) => edit(record),
                              backgroundColor: colors.surfaceContainerHighest,
                              foregroundColor: colors.onSurface,
                              label: context.tr('edit'),
                            ),
                            SlidableAction(
                              onPressed: _pending
                                  ? null
                                  : (_) => _remove(record),
                              backgroundColor: colors.error,
                              foregroundColor: colors.onError,
                              label: context.tr('delete'),
                            ),
                          ],
                        ),
                        child: Surface(
                          radius: 14,
                          padding: EdgeInsets.zero,
                          child: ListTile(
                            leading: const AppIcon('spark'),
                            title: Text(string(record['name'])),
                            trailing: const AppIcon('chevron', size: 14),
                            onTap: _pending ? null : () => edit(record),
                          ),
                        ),
                      ),
                    ),
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
