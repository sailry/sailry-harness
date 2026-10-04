import 'package:flutter/material.dart';
import 'package:flutter_slidable/flutter_slidable.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart' show HostConnection;
import '../../ui/kit.dart';
import 'live.dart';
import 'record_form.dart';

Future<void> showSettingRecords(
  BuildContext context, {
  required String kind,
  required HostConnection host,
}) async {
  final key = GlobalKey<_RecordsState>();
  await showAppSheet(
    context,
    tr(kind),
    scroll: false,
    actions: [
      ListTile(
        leading: const AppIcon('plus'),
        title: Text(tr('add')),
        onTap: () => key.currentState?.edit(),
      ),
    ],
    child: _Records(key: key, kind: kind, host: host),
  );
}

class _Records extends StatefulWidget {
  const _Records({super.key, required this.kind, required this.host});
  final String kind;
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
      final response = await widget.host.command(
        switch (widget.kind) {
          'providers' => 'list_providers',
          'roles' => 'list_roles',
          _ => 'browse_memories',
        },
        widget.kind == 'memorySettings'
            ? {
                'project': null,
                'all_projects': true,
                'archived': false,
                'query': '',
              }
            : null,
      );
      if (mounted) {
        setState(() {
          _records = objects(response['data'])
              .map(
                (record) => widget.kind == 'memorySettings'
                    ? <String, dynamic>{'summary': record}
                    : record,
              )
              .toList();
          _error = null;
        });
      }
    } catch (error) {
      if (mounted) setState(() => _error = failure(error));
    }
  }

  Future<void> edit([Map<String, dynamic>? record]) async {
    if (widget.kind == 'memorySettings' && record != null) {
      try {
        final response = await widget.host.command('read_memory', {
          'id': object(record['summary'])['id'],
        });
        record = object(response['data']);
      } catch (error) {
        if (mounted) setState(() => _error = failure(error));
        return;
      }
      if (!mounted) return;
    }
    final saved = await showAppSheet<bool>(
      context,
      tr(record == null ? 'add' : 'edit'),
      child: RecordForm(
        host: widget.host,
        kind: widget.kind,
        record: record,
        names: {
          for (final item in _records ?? <Map<String, dynamic>>[])
            if ((widget.kind == 'memorySettings'
                    ? object(item['summary'])['id']
                    : item['id']) !=
                (widget.kind == 'memorySettings'
                    ? object(record?['summary'])['id']
                    : record?['id']))
              string(
                widget.kind == 'memorySettings'
                    ? object(item['summary'])['title']
                    : item['name'],
              ),
        },
      ),
    );
    if (saved == true && mounted) await _load();
  }

  Future<void> _remove(Map<String, dynamic> record) async {
    if (_pending) return;
    final summary = widget.kind == 'memorySettings'
        ? object(record['summary'])
        : record;
    final name = string(
      summary[widget.kind == 'memorySettings' ? 'title' : 'name'],
    );
    if (!await confirmRemoval(context, name) || !mounted) return;
    setState(() {
      _pending = true;
      _error = null;
    });
    try {
      await widget.host.command(
        switch (widget.kind) {
          'providers' => 'remove_provider',
          'roles' => 'remove_role',
          _ => 'remove_memory',
        },
        {
          widget.kind == 'providers'
                  ? 'provider'
                  : widget.kind == 'roles'
                  ? 'role'
                  : 'id':
              summary['id'],
          'expected_revision': summary['revision'],
        },
      );
      if (mounted) await _load();
    } catch (error) {
      if (mounted) setState(() => _error = failure(error, saving: true));
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
            TextButton(onPressed: _load, child: Text(tr('settingsRetry'))),
          if (_records?.isEmpty == true)
            EmptyState(icon: 'spark', message: tr('settingsEmpty')),
          SlidableAutoCloseBehavior(
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                for (final record in _records ?? <Map<String, dynamic>>[])
                  Builder(
                    builder: (context) {
                      final summary = widget.kind == 'memorySettings'
                          ? object(record['summary'])
                          : record;
                      return Padding(
                        padding: const EdgeInsets.only(bottom: 8),
                        child: ClipRRect(
                          borderRadius: BorderRadius.circular(14),
                          child: Slidable(
                            key: ValueKey(
                              'setting-${widget.kind}-${summary['id']}',
                            ),
                            groupTag: widget.kind,
                            endActionPane: ActionPane(
                              motion: const DrawerMotion(),
                              extentRatio: .46,
                              children: [
                                SlidableAction(
                                  onPressed: _pending
                                      ? null
                                      : (_) => edit(record),
                                  backgroundColor:
                                      colors.surfaceContainerHighest,
                                  foregroundColor: colors.onSurface,
                                  label: tr('edit'),
                                ),
                                SlidableAction(
                                  onPressed: _pending
                                      ? null
                                      : (_) => _remove(record),
                                  backgroundColor: colors.error,
                                  foregroundColor: colors.onError,
                                  label: tr('delete'),
                                ),
                              ],
                            ),
                            child: Surface(
                              radius: 14,
                              padding: EdgeInsets.zero,
                              child: ListTile(
                                leading: AppIcon(switch (widget.kind) {
                                  'providers' => 'spark',
                                  'roles' => 'user',
                                  _ => 'file',
                                }),
                                title: Text(
                                  string(
                                    summary[widget.kind == 'memorySettings'
                                        ? 'title'
                                        : 'name'],
                                  ),
                                ),
                                trailing: const AppIcon('chevron', size: 14),
                                onTap: _pending ? null : () => edit(record),
                              ),
                            ),
                          ),
                        ),
                      );
                    },
                  ),
              ],
            ),
          ),
        ],
      ),
    );
  }
}
