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
import 'creation_draft.dart';
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
  late final _creation = CreationDraft(
    widget.host,
    initialProject: widget.initialProject,
  );

  List<PickedAttachment> get _files =>
      _attachments.putIfAbsent(text(_creation.worktree?['id']), () => []);

  @override
  void initState() {
    super.initState();
    _creation.addListener(_changed);
  }

  void _changed() => setState(() {});

  @override
  void dispose() {
    _creation.removeListener(_changed);
    _creation.dispose();
    _draft.dispose();
    _inputFocus.dispose();
    super.dispose();
  }

  void _prompt(String prompt) {
    if (_creation.busy || _creation.uncertain || _creation.attaching) return;
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
    try {
      await _creation.send(
        hasInput: _draft.text.trim().isNotEmpty || _files.isNotEmpty,
      );
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    }
  }

  Future<void> _attach() async {
    final tree = text(_creation.worktree?['id']);
    if (_creation.attaching || tree.isEmpty) return;
    setState(() => _creation.attaching = true);
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
      if (mounted) setState(() => _creation.attaching = false);
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
          host: widget.host,
          projects: objects(widget.host.snapshot['projects']),
          worktrees: _creation.worktrees,
          project: _creation.project,
          worktree: _creation.worktree,
          providers: _creation.providers,
          config: _creation.config,
          onProject: (project) => change(() {
            _creation.project = project;
            _creation.worktree = _creation.worktrees.firstOrNull;
          }),
          onWorktree: (tree) => change(() => _creation.worktree = tree),
          onConfig: (config) => change(() {
            _creation.configure(config);
          }),
        );
      },
    ),
  );

  @override
  Widget build(BuildContext context) {
    if (_creation.created case final session?) {
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
        final editable =
            !_creation.busy && !_creation.uncertain && !_creation.attaching;
        final message = !widget.host.connected
            ? 'conversationOffline'
            : _creation.project != null && _creation.worktree == null
            ? 'resourceNoWorkspace'
            : !_creation.hasModel
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
            child: message == 'conversationEmpty' && !_creation.uncertain
                ? ConversationWelcome(onPrompt: editable ? _prompt : null)
                : !widget.host.connected || _creation.uncertain
                ? FailureState(
                    icon: 'chat',
                    message: context.tr(
                      _creation.uncertain ? 'conversationUnknown' : message,
                    ),
                    action: _creation.pendingRequest == null
                        ? null
                        : FilledButton(
                            onPressed: _creation.busy || !widget.host.connected
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
                (_creation.project == null || _creation.worktree != null) &&
                _creation.hasModel,
            attachmentsEnabled:
                widget.host.connected && _creation.worktree != null,
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
