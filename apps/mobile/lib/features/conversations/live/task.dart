import 'dart:convert';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../../../ui/theme.dart';
import '../task_row.dart';
import 'page.dart';
import 'presentation.dart';

/// A one-line presentation of committed messages; tool output stays in the turn.
String messagePreview(List<Map<String, dynamic>> entries) {
  for (final entry in entries.reversed) {
    final parts = objects(entry['parts']);
    final content = parts
        .where((part) => part['kind'] == 'text')
        .map((part) => text(part['data']))
        .join('')
        .replaceAll(RegExp(r'\s+'), ' ')
        .trim();
    if (content.isNotEmpty) return content;
    for (final part in parts) {
      if (part['kind'] == 'attachment') {
        return text(
          object(object(part['data'])['spec'])['name'],
          tr('conversationAttachment'),
        );
      }
    }
  }
  return '';
}

class ConversationTask extends StatefulWidget {
  const ConversationTask({
    super.key,
    required this.host,
    required this.session,
    required this.project,
  });
  final HostConnection host;
  final Map<String, dynamic> session;
  final Map<String, dynamic> project;

  @override
  State<ConversationTask> createState() => _ConversationTaskState();
}

class _ConversationTaskState extends State<ConversationTask> {
  String? _stamp;
  String? _preview;
  bool _failed = false;
  int _request = 0;

  @override
  void initState() {
    super.initState();
    _refresh();
  }

  @override
  void didUpdateWidget(ConversationTask oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.host != widget.host ||
        oldWidget.session['id'] != widget.session['id']) {
      _stamp = null;
      _preview = null;
    }
    _refresh();
  }

  void _refresh() {
    // Unrelated host metrics and theme updates must not reread every conversation.
    final stamp = jsonEncode([
      widget.host.connected,
      widget.session['revision'],
      widget.session['activity'],
    ]);
    if (stamp == _stamp) return;
    _stamp = stamp;
    final request = ++_request;
    _failed = false;
    if (!widget.host.connected) return;
    _read(request);
  }

  Future<void> _read(int request) async {
    try {
      final host = widget.host;
      final session = widget.session['id'];
      final result = await host.command('read_conversation', {
        'session': session,
        'before': null,
        'limit': 1,
      });
      final history = object(result['data']);
      final page = object(history['page']);
      final entries = objects(page['entries']);
      var preview = messagePreview(entries);
      // Large tool-heavy turns may put the last message in an earlier chunk.
      // Follow the Node's read cursor only until that message is available.
      if (preview.isEmpty && (history['missing'] as List? ?? []).isNotEmpty) {
        final turn = (history['missing'] as List).last;
        var before = entries
            .where((entry) => entry['turn'] == turn)
            .firstOrNull?['sequence'];
        while (mounted && request == _request) {
          final result = await host.command('read_turn', {
            'session': session,
            'turn': turn,
            'expected_revision': page['revision'],
            'before': before,
            'limit': 100,
          });
          final chunk = object(result['data']);
          preview = messagePreview(objects(chunk['entries']));
          final next = chunk['next_before'];
          if (preview.isNotEmpty || next == null) break;
          if (before != null && number(next) >= number(before)) {
            throw const FormatException('message preview did not advance');
          }
          before = next;
        }
      }
      if (mounted && request == _request) {
        setState(() => _preview = preview);
      }
    } catch (error) {
      debugPrint('Conversation preview unavailable: $error');
      if (mounted && request == _request) setState(() => _failed = true);
    }
  }

  @override
  Widget build(BuildContext context) => TaskRow(
    unread:
        object(object(widget.session['activity'])['attention'])['unread'] ==
        true,
    loading:
        widget.host.connected &&
        (status(widget.session) == 'running' ||
            object(object(widget.session['activity'])['run'])['status'] ==
                'queued' ||
            number(object(widget.session['activity'])['queued']) > 0),
    icon: ProjectIcon(project: widget.project, size: 26),
    title: title(widget.session),
    preview: _preview?.isNotEmpty == true
        ? _preview!
        : _failed
        ? tr('conversationPreviewUnavailable')
        : _preview != null
        ? tr('conversationNoMessages')
        : widget.host.connected
        ? ''
        : tr('conversationOffline'),
    status: widget.host.connected ? label(widget.session) : tr('offline'),
    tone: widget.host.connected ? tone(widget.session) : StatusTone.neutral,
    onTap: () => pushPage(
      context,
      LiveConversationPage(
        host: widget.host,
        sessionId: widget.session['id'] as String,
        initialSession: widget.session,
      ),
    ),
  );
}
