import 'package:flutter/material.dart';
import '../../l10n/strings.dart';
import '../../runtime/session.dart' show HostConnection;
import 'live.dart';
import '../../ui/form.dart';

class RecordForm extends StatefulWidget {
  const RecordForm({
    super.key,
    required this.host,
    required this.kind,
    this.record,
    this.names = const {},
  });
  final HostConnection host;
  final String kind;
  final Map<String, dynamic>? record;
  final Set<String> names;
  @override
  State<RecordForm> createState() => _RecordFormState();
}

class _RecordFormState extends State<RecordForm> {
  final _form = GlobalKey<FormState>();
  final _fields = <String, TextEditingController>{};
  late final _original = widget.record ?? <String, dynamic>{};
  late final _summary = memory ? object(_original['summary']) : _original;
  late final String _id = string(_summary['id']).isEmpty
      ? newId()
      : string(_summary['id']);
  late String _api = string(_original['api']).isEmpty
      ? 'chat_completions'
      : string(_original['api']);
  late String _memoryKind = string(_summary['kind']).isEmpty
      ? 'user'
      : string(_summary['kind']);
  late bool _enabled = _original['enabled'] != false;
  bool _busy = false;
  bool _keyReady = false;
  String? _error;
  String _savedKey = '';
  String? get fixedEndpoint => switch (_api) {
    'open_code_go' => 'https://opencode.ai/zen/go/v1',
    'open_code_zen' => 'https://opencode.ai/zen/v1',
    _ => null,
  };
  bool get provider => widget.kind == 'providers';
  bool get memory => widget.kind == 'memorySettings';
  bool get keyAuth =>
      string(_original['authentication']).isEmpty ||
      _original['authentication'] == 'api_key';

  TextEditingController field(String key, [String value = '']) =>
      _fields.putIfAbsent(key, () => TextEditingController(text: value));

  @override
  void initState() {
    super.initState();
    field('name', string(_summary[memory ? 'title' : 'name']));
    field('content', string(_original[memory ? 'body' : 'instructions']));
    field('key', string(_original['key']));
    field('description', string(_original['description']));
    field('endpoint', string(_original['endpoint']));
    field(
      'models',
      objects(_original['models']).map((m) => string(m['id'])).join('\n'),
    );
    final models = objects(_original['models']);
    field(
      'context',
      models.isEmpty ? '128000' : '${integer(models.first['context'])}',
    );
    field(
      'output',
      models.isEmpty ? '16384' : '${integer(models.first['output'])}',
    );
    field('secret');
    if (provider && keyAuth && widget.record != null) {
      _readKey();
    } else {
      _keyReady = true;
    }
  }

  Future<void> _readKey() async {
    try {
      final output = await widget.host.command('read_provider_key', {
        'provider': _id,
        'expected_revision': integer(_original['revision']),
      });
      if (!mounted) return;
      setState(() {
        _savedKey = string(output['data']);
        field('secret').text = _savedKey;
        _keyReady = true;
        _error = null;
      });
    } catch (error) {
      if (mounted) setState(() => _error = failure(error));
    }
  }

  @override
  void dispose() {
    for (final field in _fields.values) {
      field.dispose();
    }
    super.dispose();
  }

  Future<void> _save() async {
    if (_busy || !_keyReady || !_form.currentState!.validate()) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final revision = integer(_summary['revision']);
      if (provider) {
        final previousModels = objects(_original['models']);
        final ids = field('models').text
            .split('\n')
            .map((id) => id.trim())
            .where((id) => id.isNotEmpty)
            .toSet()
            .toList();
        final models = ids
            .map(
              (id) =>
                  previousModels
                      .where((model) => model['id'] == id)
                      .firstOrNull ??
                  {
                    'id': id,
                    'context': int.parse(field('context').text),
                    'output': int.parse(field('output').text),
                    'vision': false,
                    'tools': true,
                    'reasoning': false,
                    'web_search': false,
                    'generates': [],
                    'efforts': [],
                    'custom_efforts': false,
                    'default_effort': 'default',
                  },
            )
            .toList();
        final key = field('secret').text;
        await widget.host.command('save_provider', {
          'provider': {
            ..._original,
            'id': _id,
            'revision': revision,
            'name': field('name').text.trim(),
            'api': _api,
            'authentication': _original['authentication'] ?? 'api_key',
            'endpoint': fixedEndpoint ?? field('endpoint').text.trim(),
            'enabled': _enabled,
            'models': models,
            'default_model': ids.contains(_original['default_model'])
                ? _original['default_model']
                : ids.first,
            'credential': keyAuth && key.isEmpty
                ? null
                : _original['credential'],
          },
          'expected_revision': revision,
          'secret': keyAuth && key.isNotEmpty && key != _savedKey ? key : null,
        });
      } else if (memory) {
        await widget.host.command('put_memory', {
          'entry': {
            ..._original,
            'summary': {
              ..._summary,
              'id': _id,
              'project': _summary['project'],
              'title': field('name').text.trim(),
              'kind': _memoryKind,
              'revision': revision,
              'updated_at_ms': integer(_summary['updated_at_ms']),
              'archived': _summary['archived'] == true,
            },
            'body': field('content').text,
          },
          'expected_revision': revision,
        });
      } else {
        await widget.host.command('put_role', {
          'role': {
            ..._original,
            'id': _id,
            'revision': revision,
            'name': field('name').text.trim(),
            'key': field('key').text.trim(),
            'description': field('description').text.trim(),
            'instructions': field('content').text,
            'model': _original['model'],
            'max_turns': _original['max_turns'],
            'skills': _original['skills'] ?? [],
          },
          'expected_revision': revision,
        });
      }
      if (mounted) Navigator.pop(context, true);
    } catch (error) {
      if (mounted) setState(() => _error = failure(error, saving: true));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Widget input(
    String key,
    String label, {
    int lines = 1,
    bool required = true,
    bool enabled = true,
    bool numeric = false,
  }) => TextFormField(
    key: ValueKey('settings-$key'),
    controller: field(key),
    enabled: !_busy && enabled,
    minLines: lines,
    maxLines: lines == 1 ? 1 : 8,
    keyboardType: numeric ? TextInputType.number : null,
    decoration: InputDecoration(labelText: tr(label)),
    validator: (value) {
      if (required && (value == null || value.trim().isEmpty)) {
        return tr('settingsRequired');
      }
      if (numeric && (int.tryParse(value ?? '') ?? 0) <= 0) {
        return tr('settingsRequired');
      }
      if (key == 'name' && widget.names.contains(value?.trim())) {
        return tr('configDuplicate');
      }
      return null;
    },
  );

  @override
  Widget build(BuildContext context) => Form(
    key: _form,
    child: FormBody(
      children: [
        input('name', 'configName'),
        if (provider) ...[
          SelectField<String>(
            value: _api,
            label: tr('settingsApi'),
            options:
                [
                      'chat_completions',
                      'responses',
                      'anthropic',
                      'gemini',
                      'deepseek',
                      'open_code_go',
                      'open_code_zen',
                      'azure_open_ai',
                      'azure_ai',
                      'bedrock',
                      'vertex',
                    ]
                    .map(
                      (api) => (
                        api,
                        switch (api) {
                          'open_code_go' => tr('settingsOpenCodeGo'),
                          'open_code_zen' => tr('settingsOpenCodeZen'),
                          _ => api,
                        },
                      ),
                    )
                    .toList(),
            onChanged: _busy ? null : (value) => setState(() => _api = value),
          ),
          if (fixedEndpoint == null) input('endpoint', 'configEndpoint'),
          if (keyAuth)
            input(
              'secret',
              'settingsApiKey',
              required: false,
              enabled: _keyReady,
            ),
          if (!_keyReady && _error != null)
            TextButton(onPressed: _readKey, child: Text(tr('settingsRetry'))),
          input('models', 'settingsModels', lines: 3),
          SwitchListTile.adaptive(
            contentPadding: EdgeInsets.zero,
            title: Text(tr('settingsEnabled')),
            value: _enabled,
            onChanged: _busy
                ? null
                : (value) => setState(() => _enabled = value),
          ),
        ] else ...[
          if (!memory) ...[
            input('key', 'settingsKey'),
            input('description', 'settingsDescription'),
          ],
          if (memory)
            SelectField<String>(
              value: _memoryKind,
              label: tr('settingsMemoryKind'),
              options: [
                for (final item in [
                  ('user', 'settingsMemoryUser'),
                  ('feedback', 'settingsMemoryFeedback'),
                  ('project', 'settingsMemoryProject'),
                  ('reference', 'settingsMemoryReference'),
                ])
                  (item.$1, tr(item.$2)),
              ],
              onChanged: _busy
                  ? null
                  : (value) => setState(() => _memoryKind = value),
            ),
          input(
            'content',
            memory ? 'configContent' : 'settingsInstructions',
            lines: 5,
          ),
        ],
        if (_error != null) settingsError(_error),
        FilledButton(
          onPressed: _busy || !_keyReady ? null : _save,
          child: Text(tr('save')),
        ),
      ],
    ),
  );
}
