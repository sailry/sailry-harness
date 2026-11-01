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
import '../../../ui/kit.dart';
import '../../../ui/toast.dart';
import '../../resources/file_navigation.dart';
import '../../resources/file_page.dart';
import 'presentation.dart';
import 'tool_record.dart';
import 'tool_display.dart';
export 'tool_record.dart' show ToolRecord;
import 'activity.dart';
import 'tool_heading.dart';
import 'tool_row.dart';
import 'turn_frame.dart';
import 'children.dart';
import 'connection_status.dart';
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
import 'publications.dart';
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
    required this.updates,
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
  final ToolSelectionChanged? toolOutput;

  /// The live projection, followed by sheets opened from the transcript.
  final ValueListenable<Map<String, dynamic>> updates;

  @override
  State<LiveTimeline> createState() => _LiveTimelineState();
}

class _LiveTimelineState extends State<LiveTimeline> {
  final _documents = <String, FileDocument>{};

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
      bool interactive = true,
      bool selectable = true,
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
              selectable: selectable,
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
          // Reasoning stays out of the mobile transcript; tools merge around it.
          return const SizedBox.shrink();
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
            key: ValueKey((
              'tool',
              entry['id'],
              index,
              interactive ? 'card' : 'row',
            )),
            documents: _documents,
            call: call,
            page: page,
            session: session,
            host: host,
            command: command,
            enabled: connected,
            continuing: continuing,
            interactive: interactive,
            toolOutput: widget.toolOutput,
            updates: widget.updates,
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
      final split = visible.lastIndexWhere((row) => row.$3['work'] == true) + 1;
      final latestTurn =
          run == runs.where((run) => run['kind'] != 'compaction').lastOrNull;
      // Older settled answers trade text selection for long-press actions.
      final pressable = !active && !latestTurn && onTurnAction != null;
      final steps = <Widget>[];
      final answerParts = <Widget>[];
      final cards = <Widget>[];
      Widget? live;
      var count = 0;
      final progressCall = calls
          .where(
            (call) => call['turn'] == run['turn'] && call['progress'] != null,
          )
          .lastOrNull;
      var progressShown = false;
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
        final continuing = continuingTools && row == last;
        final work = position < split && value['kind'] != 'image';
        final pending = work && call != null && pendingTool(call);
        final childRuns = [
          for (final reference in childReferences)
            objects(page['children'])[reference['index'] as int],
        ];
        final rendered = call?['progress'] != null
            ? ToolHeading(
                key: ValueKey('progress-${run['turn']}'),
                call: progressCall!,
                page: page,
                running: continuingTools && lastCall['progress'] != null,
                onTap: () => showAppSheet(
                  context,
                  toolTitle(context, progressCall),
                  child: ValueListenableBuilder(
                    valueListenable: widget.updates,
                    builder: (context, view, _) => TaskProgress(
                      progress:
                          turnProgress(objects(view['calls']), run['turn']) ??
                          object(progressCall['progress']),
                    ),
                  ),
                ),
              )
            : childReferences.isNotEmpty
            ? Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  for (final (position, reference) in childReferences.indexed)
                    ChildTask(
                      child: childRuns[position],
                      continuing: continuing,
                      title: text(
                        object(object(value['data'])['summary'])['title'],
                        text(
                          childRuns[position]['name'],
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
                continuing: continuing,
                interactive: !pending,
                selectable: !pressable,
              );
        if (call?['progress'] != null) progressShown = true;
        if (rendered is SizedBox) continue;
        if (!work) {
          answerParts.add(rendered);
          continue;
        }
        steps.add(rendered);
        if (call == null) continue;
        count += childReferences.isEmpty ? 1 : childReferences.length;
        if (!active) continue;
        if (pending) {
          // Decisions stay actionable below the answer while steps collapse.
          cards.add(part(entry, value, index, false, continuing: continuing));
        } else if (call['state'] == 'running' ||
            continuing ||
            childRuns.any((child) => activeRun(object(child['run'])))) {
          live = rendered;
        }
      }
      final copy = rows
          .where((entry) => entry['author'] != 'user')
          .expand((entry) => objects(entry['parts']))
          .where((part) => part['kind'] == 'text')
          .map((part) => text(part['data']))
          .join('\n\n');
      final canEdit =
          canChangeHistory &&
          !noticeTurn(run) &&
          inputs.any((input) => input.canonical);
      final canRewind =
          canChangeHistory && !noticeTurn(run) && run != runs.last;
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
            steps: steps,
            count: count,
            answer: [
              if (pressable && answerParts.isNotEmpty)
                GestureDetector(
                  behavior: HitTestBehavior.opaque,
                  onLongPress: () => showTurnActions(
                    context,
                    copy: copy,
                    canEdit: canEdit,
                    canRewind: canRewind,
                    onAction: (action) =>
                        onTurnAction(action, run['turn'] as String),
                  ),
                  child: Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: answerParts,
                  ),
                )
              else
                ...answerParts,
            ],
            current: cards.isEmpty
                ? live
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: cards,
                  ),
            phase: phase,
          ),
          PublicationDetails(
            key: ValueKey('publications-${run['turn']}'),
            calls: calls.where((call) => call['turn'] == run['turn']).toList(),
            enabled: connected,
            load: widget.toolOutput,
            builder: (context, published) => Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                PublishedFiles(
                  key: ValueKey('files-${run['turn']}'),
                  files: publishedFiles(published, run['turn'] as String),
                  host: host,
                  worktree: turnWorktree(page, session, run['turn']),
                  enabled: connected,
                  documents: _documents,
                ),
                PublishedArtifacts(
                  key: ValueKey('artifacts-${run['turn']}'),
                  artifacts: publishedArtifacts(
                    published,
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
              ],
            ),
          ),
          if (!active &&
              !noticeTurn(run) &&
              !widget.sending &&
              latestTurn &&
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
          if (run['error'] != null)
            Padding(
              padding: const EdgeInsets.only(bottom: 12),
              child: SelectableText(
                key: PageStorageKey('run-error-${run['turn']}'),
                object(run['error'])['message'] as String? ??
                    context.tr('conversationFailedStatus'),
                style: TextStyle(
                  fontSize: transcriptFontSize,
                  height: 1.7,
                  color: colors.error,
                ),
              ),
            ),
          if (!active && latestTurn)
            TurnActions(
              canEdit: canEdit,
              copy: copy,
              failed:
                  !noticeTurn(run) &&
                  ['failed', 'interrupted'].contains(run['status']),
              canRewind: canRewind,
              onAction: onTurnAction == null
                  ? null
                  : (action) => onTurnAction(action, run['turn'] as String),
            ),
        ],
      );
    }

    return VisualScope(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          for (final run in runs)
            KeyedSubtree(
              key:
                  turnKey?.call(run['turn'] as String) ?? ValueKey(run['turn']),
              child: turn(run),
            ),
          // Undelivered results wait after the latest turn for the next one.
          BackgroundNotices(
            items: pendingBackground(page),
            children: children,
            pending: true,
            onOpenChild: onOpenChild,
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
