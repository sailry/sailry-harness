import 'package:flutter/foundation.dart' show ValueListenable;
import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../user_message.dart';
import '../../../content/markdown.dart';
import '../../../content/attachment.dart';
import '../../../content/images.dart';
import '../../../content/paths.dart';
import '../../../content/visuals.dart';
import '../../../ui/toast.dart';
import '../../resources/file_navigation.dart';
import '../../resources/file_page.dart';
import 'presentation.dart';
import 'tool_record.dart';
import 'tool_sequence.dart';
import 'tool_display.dart';
export 'tool_record.dart' show ToolRecord;
import 'disclosure.dart';
import 'activity.dart';
import 'turn_frame.dart';
import 'connection_status.dart';
import 'children.dart';
import 'background.dart';
import 'background_data.dart';
import 'progress.dart';
import 'sources.dart';
import 'references.dart';
import 'message_data.dart';
import 'turn_actions.dart';
import 'turn_changes.dart';
import 'files.dart';
import 'artifacts.dart';
import '../../resources/artifact/submission.dart';

/// Renders complete Rust Client projections. Call association and streaming
/// replacement are owned by Client, never reconstructed by the mobile UI.
class LiveTimeline extends StatefulWidget {
  const LiveTimeline({
    super.key,
    required this.view,
    required this.session,
    required this.host,
    required this.command,
    this.onOpenChild,
    this.onTurnAction,
    this.turnKey,
    this.canRestore,
    this.canChangeHistory = true,
    this.sending = false,
    this.artifactActions,
    this.artifactChanges,
    this.toolOutput,
  });
  final Map<String, dynamic> view;
  final Map<String, dynamic> session;
  final HostConnection host;
  final ConversationCommand command;
  final ValueChanged<Map<String, dynamic>>? onOpenChild;
  final void Function(String action, String turn)? onTurnAction;
  final Key Function(String turn)? turnKey;
  final bool Function()? canRestore;
  final bool canChangeHistory;
  final bool sending;
  final ArtifactActions? artifactActions;
  final ValueListenable<Map<String, dynamic>>? artifactChanges;
  final OutputLoader? toolOutput;

  @override
  State<LiveTimeline> createState() => _LiveTimelineState();
}

class _LiveTimelineState extends State<LiveTimeline> {
  final _documents = <String, FileDocument>{};
  final _toolKeys = <(String, int), GlobalKey>{};

  @override
  Widget build(BuildContext context) {
    final view = widget.view;
    final session = widget.session;
    final host = widget.host;
    final command = widget.command;
    final onOpenChild = widget.onOpenChild;
    final onTurnAction = widget.onTurnAction;
    final turnKey = widget.turnKey;
    final canRestore = widget.canRestore;
    final canChangeHistory = widget.canChangeHistory;
    final snapshot = object(view['snapshot']);
    final page = object(snapshot['page']);
    final entries = objects(page['entries']);
    final calls = objects(view['calls']);
    final children = object(object(view['children'])['sessions']);
    final sources = {
      for (final call in calls)
        (
          object(call['source'])['entry'] as String,
          object(call['source'])['index'] as int,
        ),
    };
    _toolKeys.removeWhere((source, _) => !sources.contains(source));
    final drafts = objects(snapshot['drafts']);
    final runs = objects(page['runs']);
    final colors = Theme.of(context).colorScheme;
    final connected = host.connected && view['connected'] == true;
    Widget part(
      Map<String, dynamic> entry,
      Map<String, dynamic> part,
      int index,
      bool retrying, {
      bool continuing = false,
    }) {
      final data = part['data'];
      switch (part['kind']) {
        case 'text':
          final worktree =
              (runs
                          .where((run) => run['turn'] == entry['turn'])
                          .firstOrNull?['worktree'] ??
                      session['worktree'])
                  as String;
          return Padding(
            padding: const EdgeInsets.only(bottom: 16),
            child: MarkdownContent(
              data as String,
              onOpenFile: (uri) async {
                final tree = objects(
                  host.snapshot['worktrees'],
                ).where((tree) => tree['id'] == worktree).firstOrNull;
                final path = resourcePath(
                  uri,
                  tree?['path'] as String?,
                  lineReference: true,
                );
                if (path == null) {
                  showToast(context, context.tr('fileLinkUnavailable'));
                  return;
                }
                await openResourceFile(
                  context,
                  host,
                  worktree,
                  path,
                  documents: _documents,
                );
              },
              imageBuilder: (uri, _, _) => contentImage(host, worktree, uri),
            ),
          );
        case 'thinking':
          return WorkDisclosure(
            key: PageStorageKey('thought-${entry['id']}-$index'),
            title: Text(
              context.tr('thought'),
              style: TextStyle(color: colors.onSurfaceVariant),
            ),
            autoExpanded: false,
            child: Align(
              alignment: Alignment.centerLeft,
              child: MarkdownContent(
                data as String,
                key: const PageStorageKey('thought-text'),
                muted: true,
              ),
            ),
          );
        case 'compaction':
          return ExpansionTile(
            key: PageStorageKey('summary-${entry['id']}-$index'),
            tilePadding: EdgeInsets.zero,
            title: Text(context.tr('conversationCompacted')),
            children: [
              MarkdownContent(
                data as String,
                key: const PageStorageKey('summary-text'),
              ),
            ],
          );
        case 'attachment':
          final attachment = object(data);
          return AttachmentView(host: host, attachment: attachment);
        case 'image':
          final image = object(data);
          return AttachmentView(
            key: ValueKey((entry['id'], index, image['part'], image['index'])),
            host: host,
            attachment: object(image['attachment']),
            image: image,
            session: session['id'] as String,
          );
        case 'resource':
          final resource = object(data);
          if (resource['type'] == 'compaction_failure') {
            return ErrorDetails(
              key: PageStorageKey('compaction-error-${entry['id']}-$index'),
              reason: text(
                object(resource['failure'])['cause'],
                context.tr('conversationFailedStatus'),
              ),
            );
          }
          if (resource['type'] != 'model_retry') return const SizedBox.shrink();
          return Padding(
            padding: const EdgeInsets.only(bottom: 8),
            child: ActivityText(
              context
                  .tr(
                    retrying
                        ? 'conversationModelRetrying'
                        : 'conversationModelRetried',
                  )
                  .replaceAll('{attempt}', '${resource['attempt']}')
                  .replaceAll('{limit}', '${resource['limit']}'),
              active: retrying,
            ),
          );
        case 'tool_call':
        case 'tool_result':
          final call = calls.where((call) {
            final source = object(call['source']);
            return source['entry'] == entry['id'] && source['index'] == index;
          }).firstOrNull;
          if (call == null) return const SizedBox.shrink();
          return ToolRecord(
            documents: _documents,
            // A call can move from a single row into a growing tool sequence.
            key: _toolKeys.putIfAbsent((
              entry['id'] as String,
              index,
            ), GlobalKey.new),
            call: call,
            page: page,
            session: session,
            host: host,
            command: command,
            enabled: connected,
            continuing: continuing,
            toolOutput: widget.toolOutput,
          );
        case 'reference':
          return MessageReferences(
            documents: _documents,
            references: [object(data)],
            host: host,
            worktree: turnWorktree(page, session, entry['turn']),
          );
        default:
          return const SizedBox.shrink();
      }
    }

    Widget turn(Map<String, dynamic> run) {
      final rows = [
        ...entries.where((entry) => entry['turn'] == run['turn']),
        ...drafts.where((entry) => entry['turn'] == run['turn']),
      ];
      final active = activeRun(run);
      final visible = displayRows(view, run['turn']);
      final inputs = turnInputs(view, run['turn']);
      final last = visible.lastOrNull;
      final waiting = calls.any(
        (call) =>
            call['turn'] == run['turn'] &&
            (object(call['approval'])['state'] == 'pending' ||
                object(object(call['question'])['state'])['kind'] == 'pending'),
      );
      final running = calls.any(
        (call) => call['turn'] == run['turn'] && call['state'] == 'running',
      );
      final lastCall = last == null
          ? null
          : calls
                .where(
                  (call) =>
                      object(call['source'])['entry'] == last.$1['id'] &&
                      object(call['source'])['index'] == last.$2,
                )
                .firstOrNull;
      final continuingTools =
          connected &&
          run['status'] == 'running' &&
          !waiting &&
          lastCall != null &&
          ['tool_call', 'tool_result'].contains(last?.$3['kind']) &&
          object(lastCall['question']).isEmpty;
      final phase = last != null && _modelRetry(last.$3)
          ? null
          : run['status'] == 'queued'
          ? 'conversationQueued'
          : run['status'] == 'stopping'
          ? 'conversationStopping'
          : run['kind'] == 'compaction'
          ? 'conversationCompacting'
          : waiting
          ? 'conversationWaiting'
          : continuingTools ||
                running ||
                children.values
                    .map(
                      (reference) => objects(
                        page['children'],
                      )[object(reference)['index'] as int],
                    )
                    .any(
                      (child) =>
                          object(child['origin'])['turn'] == run['turn'] &&
                          activeRun(object(child['run'])),
                    )
          ? null
          : last?.$3['kind'] == 'text'
          ? 'conversationGenerating'
          : 'thinkingNow';
      final split =
          visible.lastIndexWhere(
            (row) =>
                ![
                  'text',
                  'image',
                  'attachment',
                  'reference',
                ].contains(row.$3['kind']) &&
                !calls.any(
                  (call) =>
                      call['terminal_interaction'] == true &&
                      ((object(call['source'])['entry'] == row.$1['id'] &&
                              object(call['source'])['index'] == row.$2) ||
                          (object(call['response'])['entry'] == row.$1['id'] &&
                              object(call['response'])['index'] == row.$2)),
                ),
          ) +
          1;
      final work = <Widget>[];
      final answerParts = <Widget>[];
      final latestProgress = calls
          .where(
            (call) => call['turn'] == run['turn'] && call['progress'] != null,
          )
          .lastOrNull;
      var progressShown = false;
      final sequence = <ToolRecord>[];
      void flushTools() {
        if (sequence.isEmpty) return;
        final source = object(sequence.first.call['source']);
        work.add(
          sequence.length == 1
              ? sequence.single
              : ToolSequence(
                  key: ValueKey(
                    'tool-sequence-${run['turn']}-${source['entry']}-${source['index']}',
                  ),
                  tools: List.of(sequence),
                ),
        );
        sequence.clear();
      }

      for (final (position, row) in visible.indexed) {
        final (entry, index, value) = row;
        final call = calls
            .where(
              (call) =>
                  object(call['source'])['entry'] == entry['id'] &&
                  object(call['source'])['index'] == index,
            )
            .firstOrNull;
        final childReferences = objects(call?['children']);
        if (call?['progress'] != null && progressShown) continue;
        final rendered = call?['progress'] != null
            ? TaskProgress(
                key: PageStorageKey('progress-${run['turn']}'),
                progress: object(latestProgress!['progress']),
                continuing: continuingTools && lastCall['progress'] != null,
              )
            : childReferences.isNotEmpty
            ? Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final reference in childReferences)
                    ChildTask(
                      child: objects(
                        page['children'],
                      )[reference['index'] as int],
                      continuing: continuingTools && row == last,
                      title: text(
                        object(object(value['data'])['arguments'])['title'],
                        text(
                          objects(page['children'])[reference['index']
                              as int]['name'],
                          context.tr('conversationChild'),
                        ),
                      ),
                      connected: connected,
                      onOpen:
                          onOpenChild == null ||
                              childSession(host, reference) == null
                          ? null
                          : () => onOpenChild(reference),
                    ),
                ],
              )
            : part(
                entry,
                value,
                index,
                active && connected && row == last,
                continuing: continuingTools && row == last,
              );
        if (call?['progress'] != null) progressShown = true;
        if (rendered is SizedBox) continue;
        if (position < split &&
            rendered is ToolRecord &&
            object(call?['question']).isEmpty &&
            call?['grouping'] != 'standalone' &&
            call?['presentation'] != 'progress') {
          sequence.add(rendered);
          continue;
        }
        flushTools();
        if (position >= split || value['kind'] == 'image') {
          answerParts.add(rendered);
        } else {
          work.add(rendered);
        }
      }
      flushTools();
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          BackgroundNotices(
            items: objects(run['notices']),
            children: children,
            onOpenChild: onOpenChild,
          ),
          for (final input in inputs)
            UserBubble(
              onEdit:
                  onTurnAction == null || !canChangeHistory || !input.canonical
                  ? null
                  : () => onTurnAction('edit', run['turn'] as String),
              text: input.text,
              attachment: input.attachments.isEmpty && input.references.isEmpty
                  ? null
                  : Column(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        if (input.references.isNotEmpty)
                          MessageReferences(
                            documents: _documents,
                            host: host,
                            worktree: turnWorktree(page, session, run['turn']),
                            references: input.references,
                          ),
                        for (final attachment in input.attachments)
                          AttachmentView(host: host, attachment: attachment),
                      ],
                    ),
            ),
          TurnFrame(
            key: ValueKey('turn-${run['turn']}'),
            run: run,
            connected: connected,
            work: work,
            answer: answerParts,
            phase: phase,
          ),
          PublishedFiles(
            key: ValueKey('files-${run['turn']}'),
            files: publishedFiles(calls, run['turn'] as String),
            host: host,
            worktree: turnWorktree(page, session, run['turn']),
            enabled: connected,
            documents: _documents,
          ),
          PublishedArtifacts(
            key: ValueKey('artifacts-${run['turn']}'),
            artifacts: publishedArtifacts(
              calls,
              run['turn'] as String,
              session['id'] as String,
              origin: run['origin'] as String?,
            ),
            host: host,
            enabled: connected,
            session: session['id'] as String,
            actions: widget.artifactActions,
            changes: widget.artifactChanges,
          ),
          if (!active &&
              !noticeTurn(run) &&
              !widget.sending &&
              run ==
                  runs.where((run) => run['kind'] != 'compaction').lastOrNull &&
              calls.any((call) => call['turn'] == run['turn']))
            TurnChanges(
              key: ValueKey('changes-${run['turn']}'),
              host: host,
              canRestore: canRestore,
              session: session['id'] as String,
              turn: run['turn'] as String,
              worktree: turnWorktree(page, session, run['turn']),
            ),
          for (final entry in rows.where((entry) => entry['author'] != 'user'))
            EntrySources(key: ValueKey('sources-${entry['id']}'), entry: entry),
          if (!active)
            TurnActions(
              canEdit:
                  canChangeHistory &&
                  !noticeTurn(run) &&
                  inputs.any((input) => input.canonical),
              copy: rows
                  .where((entry) => entry['author'] != 'user')
                  .expand((entry) => objects(entry['parts']))
                  .where((part) => part['kind'] == 'text')
                  .map((part) => text(part['data']))
                  .join('\n\n'),
              failed:
                  !noticeTurn(run) &&
                  ['failed', 'interrupted'].contains(run['status']),
              canRewind:
                  canChangeHistory && !noticeTurn(run) && run != runs.last,
              onAction: onTurnAction == null
                  ? null
                  : (action) => onTurnAction(action, run['turn'] as String),
            ),
          if (run['error'] != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 16),
              child: ErrorDetails(
                key: PageStorageKey('run-error-${run['turn']}'),
                reason:
                    object(run['error'])['message'] as String? ??
                    context.tr('conversationFailedStatus'),
              ),
            ),
        ],
      );
    }

    return VisualScope(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          BackgroundNotices(
            items: pendingBackground(page),
            children: children,
            pending: true,
            onOpenChild: onOpenChild,
          ),
          for (final run in runs)
            KeyedSubtree(
              key:
                  turnKey?.call(run['turn'] as String) ?? ValueKey(run['turn']),
              child: turn(run),
            ),
        ],
      ),
    );
  }
}

bool _modelRetry(Map<String, dynamic> part) {
  final data = object(part['data']);
  return part['kind'] == 'resource' && data['type'] == 'model_retry';
}
