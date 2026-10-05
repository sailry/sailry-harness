import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../conversation_frame.dart';
import '../message_composer.dart';
import '../welcome.dart';
import 'attachments.dart';
import 'draft_attachments.dart';
import 'draft_settings.dart';
import 'page.dart';
import 'presentation.dart' show failureLabel;
import 'voice.dart';

/// A local draft becomes a Node session only when the first message is sent.
class NewConversationPage extends StatefulWidget {
  const NewConversationPage({
    super.key,
    required this.host,
    this.initialProject,
  });
  final HostConnection host;
  final String? initialProject;

  @override
  State<NewConversationPage> createState() => _NewConversationPageState();
}

class _NewConversationPageState extends State<NewConversationPage> {
  final _draft = TextEditingController();
  final _inputFocus = FocusNode();
  final _attachments = <String, List<PickedAttachment>>{};
  Map<String, dynamic>? _project;
  Map<String, dynamic>? _worktree;
  Map<String, dynamic> _config = {};
  Map<String, dynamic>? _created;
  String? _pendingRequest;
  bool _busy = false;
  bool _uncertain = false;
  bool _attaching = false;
  bool _defaultsCaptured = false;
  bool _configChosen = false;

  List<Map<String, dynamic>> get _providers =>
      objects(widget.host.snapshot['providers'])
          .where(
            (provider) =>
                provider['enabled'] == true &&
                objects(provider['models']).isNotEmpty,
          )
          .toList();

  List<Map<String, dynamic>> get _worktrees => _project == null
      ? []
      : objects(
          widget.host.snapshot['worktrees'],
        ).where((tree) => tree['project'] == _project!['id']).toList();

  List<PickedAttachment> get _files =>
      _attachments.putIfAbsent(text(_worktree?['id']), () => []);

  bool get _hasModel => _providers.any(
    (provider) =>
        provider['id'] == _config['provider'] &&
        objects(
          provider['models'],
        ).any((model) => model['id'] == _config['model']),
  );

  @override
  void initState() {
    super.initState();
    _syncDraft();
    widget.host.addListener(_hostChanged);
  }

  void _hostChanged() {
    if (!mounted || _busy || _uncertain || _attaching || _created != null) {
      return;
    }
    setState(_syncDraft);
  }

  void _syncDraft() {
    final projects = objects(widget.host.snapshot['projects']);
    _project =
        projects
            .where((project) => project['id'] == _project?['id'])
            .firstOrNull ??
        projects
            .where((project) => project['id'] == widget.initialProject)
            .firstOrNull ??
        projects.firstOrNull;
    _worktree =
        _worktrees
            .where((tree) => tree['id'] == _worktree?['id'])
            .firstOrNull ??
        _worktrees.firstOrNull;
    if (!_defaultsCaptured && !_configChosen) {
      final defaults = object(
        object(widget.host.snapshot['defaults'])['config'],
      );
      _config = {
        'mode': defaults['mode'] ?? 'code',
        'permission': defaults['permission'] ?? 'ask',
        'provider': defaults['provider'],
        'model': defaults['model'],
        'effort': defaults['effort'] ?? 'default',
        'credential': defaults['credential'],
      };
      _defaultsCaptured = widget.host.snapshot.containsKey('defaults');
    }
    if (!_hasModel && !_configChosen) {
      final provider = _providers.firstOrNull;
      if (provider != null) {
        final models = objects(provider['models']);
        final model =
            models
                .where((model) => model['id'] == provider['default_model'])
                .firstOrNull ??
            models.first;
        _config.addAll({
          'provider': provider['id'],
          'credential': provider['credential'],
          'model': model['id'],
          'effort': model['default_effort'] ?? 'default',
        });
      }
    }
  }

  @override
  void dispose() {
    widget.host.removeListener(_hostChanged);
    _draft.dispose();
    _inputFocus.dispose();
    super.dispose();
  }

  void _prompt(String prompt) {
    if (_busy || _uncertain || _attaching) return;
    final content = _draft.text.trim().isEmpty
        ? prompt
        : '${_draft.text}\n\n$prompt';
    _draft.value = TextEditingValue(
      text: content,
      selection: TextSelection.collapsed(offset: content.length),
    );
    _inputFocus.requestFocus();
  }

  Future<void> _send() async {
    if (_busy || _attaching || !widget.host.connected) return;
    if (_uncertain && _pendingRequest == null) return;
    if (!_uncertain &&
        (_project != null && _worktree == null ||
            !_hasModel ||
            _draft.text.trim().isEmpty && _files.isEmpty)) {
      return;
    }
    setState(() => _busy = true);
    try {
      final result = _uncertain
          ? await widget.host.execute(_pendingRequest!)
          : await widget.host.command('create_session', {
              'project': _project?['id'],
              'worktree': _worktree?['id'],
              'config': Map<String, dynamic>.of(_config),
            });
      if (mounted) setState(() => _created = object(result['data']));
    } catch (error) {
      if (mounted) {
        if (error is CommandFailure && error.code == 'outcome_unknown') {
          _uncertain = true;
          _pendingRequest = error.request ?? _pendingRequest;
        }
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _attach() async {
    final tree = text(_worktree?['id']);
    if (_attaching || tree.isEmpty) return;
    setState(() => _attaching = true);
    try {
      final file = await pickAttachment(widget.host, tree);
      if (mounted && file != null) {
        setState(() => _attachments.putIfAbsent(tree, () => []).add(file));
      }
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _attaching = false);
    }
  }

  void _settings() => showAppSheet(
    context,
    context.tr('modelPicker'),
    child: StatefulBuilder(
      builder: (context, update) {
        void change(VoidCallback action) {
          setState(action);
          update(() {});
        }

        return DraftSettings(
          projects: objects(widget.host.snapshot['projects']),
          worktrees: _worktrees,
          project: _project,
          worktree: _worktree,
          providers: _providers,
          config: _config,
          onProject: (project) => change(() {
            _project = project;
            _worktree = _worktrees.firstOrNull;
          }),
          onWorktree: (tree) => change(() => _worktree = tree),
          onConfig: (config) => change(() {
            _config = config;
            _configChosen = true;
          }),
        );
      },
    ),
  );

  @override
  Widget build(BuildContext context) {
    if (_created case final session?) {
      return LiveConversationPage(
        key: ValueKey(session['id']),
        host: widget.host,
        sessionId: session['id'] as String,
        initialSession: session,
        initialDraft: _draft.text,
        initialAttachments: List.of(_files),
        submitInitial: true,
      );
    }
    return ListenableBuilder(
      listenable: widget.host,
      builder: (context, _) {
        final editable = !_busy && !_uncertain && !_attaching;
        final message = !widget.host.connected
            ? 'conversationOffline'
            : _project != null && _worktree == null
            ? 'resourceNoWorkspace'
            : !_hasModel
            ? 'conversationNoModel'
            : 'conversationEmpty';
        return ConversationFrame(
          title: context.tr('newConversation'),
          leading: Align(
            alignment: Alignment.centerLeft,
            child: Surface(
              radius: 20,
              padding: EdgeInsets.zero,
              child: IconButton(
                style: IconButton.styleFrom(
                  fixedSize: const Size.square(36),
                  minimumSize: const Size.square(36),
                  padding: EdgeInsets.zero,
                  tapTargetSize: MaterialTapTargetSize.shrinkWrap,
                ),
                icon: const AppIcon('settings', size: 18),
                tooltip: context.tr('modelPicker'),
                onPressed: editable ? _settings : null,
              ),
            ),
          ),
          actions: const [],
          bodyBuilder: (context, bottomInset) => Padding(
            padding: EdgeInsets.only(
              top: ConversationFrame.contentInset(context),
              bottom: bottomInset,
            ),
            child: message == 'conversationEmpty' && !_uncertain
                ? ConversationWelcome(onPrompt: editable ? _prompt : null)
                : !widget.host.connected || _uncertain
                ? FailureState(
                    icon: 'chat',
                    message: context.tr(
                      _uncertain ? 'conversationUnknown' : message,
                    ),
                    action: _pendingRequest == null
                        ? null
                        : FilledButton(
                            onPressed: _busy || !widget.host.connected
                                ? null
                                : _send,
                            child: Text(context.tr('conversationCheckResult')),
                          ),
                  )
                : EmptyState(icon: 'chat', message: context.tr(message)),
          ),
          composer: MessageComposer(
            controller: _draft,
            focusNode: _inputFocus,
            attached: false,
            attachments: _files.isEmpty
                ? null
                : DraftAttachments(
                    attachments: _files,
                    onRemove: (file) {
                      if (editable) setState(() => _files.remove(file));
                    },
                  ),
            busy: false,
            enabled: editable,
            sendEnabled:
                widget.host.connected &&
                (_project == null || _worktree != null) &&
                _hasModel,
            attachmentsEnabled: widget.host.connected && _worktree != null,
            onAttachment: (_) => _attach(),
            onVoice: () => dictate(context, _draft),
            onStop: () {},
            onSend: _send,
          ),
        );
      },
    );
  }
}
