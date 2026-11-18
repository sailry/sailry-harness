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
import 'team.dart';

/// A local draft becomes a Node session only when the first message is sent.
class NewConversationPage extends StatefulWidget {
  const NewConversationPage({
    super.key,
    required this.host,
    this.initialProject,
    this.initialWorktree,
  });
  final HostConnection host;
  final String? initialProject;
  final String? initialWorktree;

  @override
  State<NewConversationPage> createState() => _NewConversationPageState();
}

class _NewConversationPageState extends State<NewConversationPage> {
  final _inputFocus = FocusNode();
  late final _creation = CreationDraft.retain(
    widget.host,
    initialProject: widget.initialProject,
    initialWorktree: widget.initialWorktree,
  );

  TextEditingController get _draft => _creation.input;
  List<PickedAttachment> get _files => _creation.files;

  @override
  void initState() {
    super.initState();
    _creation.addListener(_changed);
  }

  void _changed() => setState(() {});

  @override
  void dispose() {
    _creation.removeListener(_changed);
    _creation.release();
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

  Future<void> _send() => _settle(
    () => _creation.send(
      message: _draft.text,
      attachments: [for (final file in _files) text(file.attachment['id'])],
    ),
  );

  Future<void> _check() => _settle(_creation.check);

  Future<void> _settle(Future<void> Function() operation) async {
    try {
      await operation();
      if (mounted && _creation.unavailable) {
        _creation.clearInput();
        showToast(context, context.tr('conversationUnavailable'));
        await Navigator.maybePop(context);
      }
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    }
  }

  Future<void> _attach() async {
    final tree = text(_creation.worktree?['id']);
    if (_creation.attaching || tree.isEmpty) return;
    _creation.attaching = true;
    try {
      final file = await pickAttachment(widget.host, tree);
      if (mounted && file != null) {
        _files.add(file);
      }
    } catch (error) {
      if (mounted) {
        showToast(context, failureLabel(error, translate: context.tr));
      }
    } finally {
      _creation.attaching = false;
    }
  }

  void _settings() => showAppSheet(
    context,
    context.tr('modelPicker'),
    child: ListenableBuilder(
      listenable: _creation,
      builder: (context, _) => DraftSettings(
        host: widget.host,
        projects: objects(widget.host.snapshot['projects']),
        worktrees: _creation.worktrees,
        project: _creation.project,
        worktree: _creation.worktree,
        providers: _creation.providers,
        config: _creation.config,
        team: TeamChoices(
          listenable: _creation,
          read: () => _creation.team,
          onTeam: (team) async => _creation.selectTeam(team),
          onMember: (role, model) async =>
              _creation.configureMember(role, model),
        ),
        locationEnabled: _creation.locationEditable,
        onProject: (project) => _creation.project = project,
        onWorktree: _creation.selectWorktree,
        onConfig: _creation.configure,
      ),
    ),
  );

  @override
  Widget build(BuildContext context) {
    if (_creation.created case final session?) {
      if (ModalRoute.of(context)?.isCurrent != false) _creation.presented();
      return LiveConversationPage(
        key: ValueKey(session['id']),
        host: widget.host,
        sessionId: session['id'] as String,
        initialSession: session,
      );
    }
    return ListenableBuilder(
      listenable: widget.host,
      builder: (context, _) {
        final editable =
            !_creation.busy &&
            !_creation.uncertain &&
            !_creation.attaching &&
            !_creation.unavailable;
        final message = _creation.unavailable
            ? 'conversationUnavailable'
            : !widget.host.connected
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
                : !widget.host.connected ||
                      _creation.uncertain ||
                      _creation.unavailable
                ? FailureState(
                    icon: 'chat',
                    message: context.tr(
                      _creation.uncertain ? 'conversationUnknown' : message,
                    ),
                    action: !_creation.canCheck
                        ? null
                        : FilledButton(
                            onPressed: _creation.busy || !widget.host.connected
                                ? null
                                : _check,
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
                !widget.host.isDemo &&
                widget.host.connected &&
                _creation.worktree != null,
            onAttachment: (_) => _attach(),
            onStop: () {},
            onSend: _send,
          ),
        );
      },
    );
  }
}
