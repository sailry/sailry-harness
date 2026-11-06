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
import 'sources.dart';
import 'references.dart';
import 'turn_actions.dart';
import 'turn_changes.dart';
import 'files.dart';
import 'artifacts.dart';
import 'publications.dart';
import 'transcript_alignment.dart';
import 'transcript_index.dart';
import '../../resources/artifact/submission.dart';

/// Renders complete Rust Client projections. Call association and streaming
/// replacement are owned by Client, never reconstructed by the mobile UI.
///
/// Turns are built lazily from the newest, so opening a long conversation lays
/// out only the visible turns.
class LiveTimeline extends StatefulWidget {
  const LiveTimeline({
    super.key,
    required this.view,
    required this.controller,
    required this.padding,
    this.older = const [],
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
  final ScrollController controller;
  final EdgeInsets padding;

  /// Keyed items shown above the oldest loaded turn.
  final List<Widget> older;
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

  @override
  State<LiveTimeline> createState() => _LiveTimelineState();
}

class _LiveTimelineState extends State<LiveTimeline> {
  final _documents = <String, FileDocument>{};
  TranscriptIndex? _index;

  TranscriptIndex get _transcript {
    final index = _index;
    if (index != null && identical(index.view, widget.view)) return index;
    return _index = TranscriptIndex(widget.view);
  }

  @override
  Widget build(BuildContext context) {
    final view = widget.view;
    final transcript = _transcript;
    final session = widget.session;
    final host = widget.host;
    final command = widget.command;
    final onOpenChild = widget.onOpenChild;
    final onTurnAction = widget.onTurnAction;
    final turnKey = widget.turnKey;
    final canRestore = widget.canRestore;
    final canChangeHistory = widget.canChangeHistory;
    final page = transcript.page;
    final children = transcript.children;
    final runs = transcript.timeline;
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
          final worktree = transcript.worktree(entry['turn'], session);
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
          // The summary is model context; the transcript only marks where it happened.
          return Padding(
            key: ValueKey('summary-${entry['id']}-$index'),
            padding: const EdgeInsets.only(bottom: 8),
            child: ActivityText(
              context.tr('conversationCompacted'),
              active: false,
            ),
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
          final call = transcript.call(entry['id'], index);
          if (call == null) return const SizedBox.shrink();
          return ToolRecord(
            key: ValueKey((
              'tool',
              entry['id'],
              index,
              interactive ? 'card' : 'row',
            )),
            call: call,
            page: page,
            session: session,
            command: command,
            enabled: connected,
            continuing: continuing,
            interactive: interactive,
            toolOutput: widget.toolOutput,
          );
        case 'reference':
          return MessageReferences(
            documents: _documents,
            references: [object(data)],
            host: host,
            worktree: transcript.worktree(entry['turn'], session),
          );
        default:
          return const SizedBox.shrink();
      }
    }

    Widget turn(Map<String, dynamic> run) {
      final rows = transcript.entriesOf(run['turn']);
      final turnCalls = transcript.callsOf(run['turn']);
      final worktree = transcript.worktree(run['turn'], session);
      final active = activeRun(run);
      // Delivered input divides the reply; status and actions stay on the last.
      final segments = transcript.segmentsOf(run['turn']);
      if (segments.isEmpty) segments.add((inputs: [], rows: []));
      final last = segments.last.rows.lastOrNull;
      final waiting = turnCalls.any(
        (call) =>
            object(call['approval'])['state'] == 'pending' ||
            object(object(call['question'])['state'])['kind'] == 'pending',
      );
      final running = turnCalls.any((call) => call['state'] == 'running');
      final lastCall = last == null
          ? null
          : transcript.call(last.$1['id'], last.$2);
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
          : continuingTools || running || transcript.hasActiveChild(run['turn'])
          ? null
          : last?.$3['kind'] == 'text'
          ? 'conversationGenerating'
          : 'thinkingNow';
      final latestTurn = identical(run, transcript.latest);
      // Older settled answers trade text selection for long-press actions.
      final pressable = !active && !latestTurn && onTurnAction != null;
      final cards = <Widget>[];
      Widget? live;
      final progressCall = turnCalls
          .where((call) => call['progress'] != null)
          .lastOrNull;
      var progressShown = false;
      final copy = rows
          .where((entry) => entry['author'] != 'user')
          .expand((entry) => objects(entry['parts']))
          .where((part) => part['kind'] == 'text')
          .map((part) => text(part['data']))
          .join('\n\n');
      final canEdit =
          canChangeHistory &&
          !noticeTurn(run) &&
          segments.any(
            (segment) => segment.inputs.any((input) => input.canonical),
          );
      final canRewind =
          canChangeHistory && !noticeTurn(run) && run != transcript.runs.last;
      final body = <Widget>[];
      for (final (segmentIndex, segment) in segments.indexed) {
        final settled = segmentIndex < segments.length - 1;
        final visible = segment.rows;
        final split =
            visible.lastIndexWhere((row) => row.$3['work'] == true) + 1;
        final steps = <Widget>[];
        final answerParts = <Widget>[];
        var count = 0;
        for (final (position, row) in visible.indexed) {
          final (entry, index, value) = row;
          final call = transcript.call(entry['id'], index);
          final childReferences = objects(call?['children']);
          if (call?['progress'] != null && progressShown) continue;
          final continuing = continuingTools && row == last;
          final work = position < split && value['kind'] != 'image';
          final pending = work && call != null && pendingTool(call);
          final childRuns = [
            for (final reference in childReferences)
              transcript.childRecords[reference['index'] as int],
          ];
          final rendered = call?['progress'] != null
              ? ToolHeading(
                  key: ValueKey('progress-${run['turn']}'),
                  call: progressCall!,
                  running: continuingTools && lastCall['progress'] != null,
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
        body.addAll([
          for (final (position, input) in segment.inputs.indexed)
            UserBubble(
              key: ValueKey(('input', input.turn, segmentIndex, position)),
              onEdit:
                  onTurnAction == null || !canChangeHistory || !input.canonical
                  ? null
                  : () => onTurnAction('edit', input.turn),
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
                            worktree: transcript.worktree(input.turn, session),
                            references: input.references,
                          ),
                        for (final attachment in input.attachments)
                          AttachmentView(host: host, attachment: attachment),
                      ],
                    ),
            ),
          TurnFrame(
            key: ValueKey(
              settled
                  ? 'turn-${run['turn']}-$segmentIndex'
                  : 'turn-${run['turn']}',
            ),
            run: run,
            connected: connected,
            steps: steps,
            count: count,
            segment: segmentIndex,
            settled: settled,
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
            current: settled
                ? null
                : cards.isEmpty
                ? live
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: cards,
                  ),
            phase: settled ? null : phase,
          ),
        ]);
      }
      return Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          BackgroundNotices(
            items: objects(run['notices']),
            children: children,
            onOpenChild: onOpenChild,
          ),
          ...body,
          PublicationDetails(
            key: ValueKey('publications-${run['turn']}'),
            calls: turnCalls,
            enabled: connected,
            load: widget.toolOutput,
            builder: (context, published) => Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                PublishedFiles(
                  key: ValueKey('files-${run['turn']}'),
                  files: publishedFiles(published, run['turn'] as String),
                  host: host,
                  worktree: worktree,
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
              turnCalls.isNotEmpty)
            TurnChanges(
              key: ValueKey('changes-${run['turn']}'),
              host: host,
              canRestore: canRestore,
              session: session['id'] as String,
              turn: run['turn'] as String,
              worktree: worktree,
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

    final older = widget.older;
    // Index 0 is the bottom of the reversed transcript.
    final positions = <Key, int>{
      for (final (position, run) in runs.indexed)
        _turnItemKey(run): runs.length - 1 - position,
      for (final (position, item) in older.indexed)
        if (item.key case final Key key) key: runs.length + position,
    };
    return VisualScope(
      child: CustomScrollView(
        controller: widget.controller,
        reverse: true,
        keyboardDismissBehavior: ScrollViewKeyboardDismissBehavior.onDrag,
        slivers: [
          SliverAlignEnd(
            sliver: SliverPadding(
              padding: widget.padding,
              sliver: SliverList(
                delegate: SliverChildBuilderDelegate(
                  (context, position) {
                    if (position >= runs.length) {
                      return older[position - runs.length];
                    }
                    final run = runs[runs.length - 1 - position];
                    return KeyedSubtree(
                      key: _turnItemKey(run),
                      child: KeyedSubtree(
                        key:
                            turnKey?.call(run['turn'] as String) ??
                            ValueKey(run['turn']),
                        child: turn(run),
                      ),
                    );
                  },
                  childCount: runs.length + older.length,
                  findChildIndexCallback: (key) => positions[key],
                ),
              ),
            ),
          ),
        ],
      ),
    );
  }
}

Key _turnItemKey(Map<String, dynamic> run) => ValueKey(('turn', run['turn']));

bool _modelRetry(Map<String, dynamic> part) {
  final data = object(part['data']);
  return part['kind'] == 'resource' && data['type'] == 'model_retry';
}
