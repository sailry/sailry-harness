import 'package:flutter/material.dart';
import '../../ui/kit.dart';
import '../../l10n/strings.dart';
import '../../runtime/session.dart' show HostConnection;
import 'live.dart';
import 'setting_records.dart';

class MemorySheet extends StatefulWidget {
  const MemorySheet({super.key, required this.host});
  final HostConnection host;
  @override
  State<MemorySheet> createState() => _MemorySheetState();
}

class _MemorySheetState extends State<MemorySheet> {
  Map<String, dynamic>? _saved;
  final _budget = TextEditingController();
  final _days = TextEditingController();
  bool _enabled = true;
  bool _auto = true;
  bool _busy = false;
  String? _error;
  @override
  void initState() {
    super.initState();
    _load();
  }

  @override
  void dispose() {
    _budget.dispose();
    _days.dispose();
    super.dispose();
  }

  Future<void> _load() async {
    try {
      final response = await widget.host.command('read_memory_settings');
      if (!mounted) return;
      setState(() {
        _saved = object(response['data']);
        _enabled = _saved!['enabled'] == true;
        _auto = _saved!['auto_write'] == true;
        _budget.text = '${_saved!['context_bytes']}';
        _days.text = '${_saved!['review_after_days']}';
        _error = null;
      });
    } catch (error) {
      if (mounted) {
        setState(() => _error = failure(error, translate: context.tr));
      }
    }
  }

  Future<void> _save() async {
    if (_busy || _saved == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      await widget.host.command('save_memory_settings', {
        'settings': {
          ..._saved!,
          'enabled': _enabled,
          'auto_write': _auto,
          'context_bytes': int.parse(_budget.text),
          'review_after_days': int.parse(_days.text),
        },
      });
      if (mounted) Navigator.pop(context);
    } catch (error) {
      if (mounted) {
        setState(
          () => _error = failure(error, saving: true, translate: context.tr),
        );
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => _saved == null && _error != null
      ? FailureState(icon: 'settings', message: _error!, onRetry: _load)
      : LoadingOverlay(
          scroll: true,
          loading: _busy || _saved == null && _error == null,
          minHeight: 128,
          child: FormBody(
            children: [
              if (_saved != null) ...[
                SwitchListTile.adaptive(
                  contentPadding: EdgeInsets.zero,
                  title: Text(context.tr('settingsEnabled')),
                  value: _enabled,
                  onChanged: _busy ? null : (v) => setState(() => _enabled = v),
                ),
                SwitchListTile.adaptive(
                  contentPadding: EdgeInsets.zero,
                  title: Text(context.tr('settingsMemoryAuto')),
                  value: _auto,
                  onChanged: _busy ? null : (v) => setState(() => _auto = v),
                ),
                TextField(
                  controller: _budget,
                  enabled: !_busy,
                  keyboardType: TextInputType.number,
                  decoration: InputDecoration(
                    labelText: context.tr('settingsMemoryBudget'),
                  ),
                ),
                TextField(
                  controller: _days,
                  enabled: !_busy,
                  keyboardType: TextInputType.number,
                  decoration: InputDecoration(
                    labelText: context.tr('settingsMemoryReview'),
                  ),
                ),
                ListTile(
                  title: Text(context.tr('settingsMemoryRecords')),
                  trailing: const Icon(Icons.chevron_right),
                  onTap: () => showSettingRecords(
                    context,
                    kind: 'memorySettings',
                    host: widget.host,
                  ),
                ),
              ],
              if (_error != null) settingsError(_error),
              FilledButton(
                onPressed: _busy || _saved == null ? null : _save,
                child: Text(context.tr('save')),
              ),
            ],
          ),
        );
}
