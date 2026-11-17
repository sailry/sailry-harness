import 'dart:convert';

import 'package:flutter/material.dart';

import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import '../../../ui/project_icon.dart';
import '../task_row.dart';
import 'page.dart';
import 'presentation.dart';
import 'recency.dart';

/// A one-line presentation of committed messages; tool output stays in the turn.
String messagePreview(
  List<Map<String, dynamic>> entries, {
  Translator translate = tr,
}) {
  for (final entry in entries.reversed) {
    final parts = objects(entry['parts']);
    final content = StringBuffer();
    var length = 0;
    var space = false;
    // A list row needs only a short prefix, even when a reply contains a large
    // document. Preserve part boundaries and Unicode without joining the reply.
    for (final part in parts) {
      if (part['kind'] != 'text') continue;
      for (final rune in text(part['data']).runes) {
        if (_previewWhitespace.hasMatch(String.fromCharCode(rune))) {
          space = length > 0;
          continue;
        }
        if (space) {
          content.write(' ');
          length++;
          space = false;
        }
        if (length >= 512) return content.toString().trimRight();
        content.writeCharCode(rune);
        if (++length == 512) return content.toString();
      }
    }
    if (content.isNotEmpty) return content.toString();
    for (final part in parts) {
      if (part['kind'] == 'attachment') {
        return text(
          object(object(part['data'])['spec'])['name'],
          translate('conversationAttachment'),
        );
      }
    }
  }
  return '';
}

final _previewWhitespace = RegExp(r'\s');

class ConversationTask extends StatefulWidget {
  const ConversationTask({
    super.key,
    required this.host,
    required this.session,
    required this.project,
    required this.now,
    this.divider = true,
    this.previewActive,
  });
  final HostConnection host;
  final Map<String, dynamic> session;
  final Map<String, dynamic> project;

  /// Shared page clock so row times and list sections agree.
  final DateTime now;
  final bool divider;

  /// A covered task list keeps its rows without continuing background reads.
  final bool Function()? previewActive;

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
    if (widget.previewActive?.call() == false) return;
    // Read markers, titles and configuration revisions do not change history.
    // Run transitions and queue/wait changes still refresh the committed preview.
    final activity = object(widget.session['activity']);
    final run = object(activity['run']);
    final stamp = jsonEncode([
      widget.host.connected,
      run['turn'],
      run['sequence'],
      run['revision'],
      run['status'],
      run['finished_ms'],
      activity['waiting'],
      activity['queued'],
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
      if (!mounted || request != _request) return;
      final page = object(history['page']);
      final entries = objects(page['entries']);
      var preview = messagePreview(entries, translate: context.tr);
      // Large tool-heavy turns may put the last message in an earlier chunk.
      // Follow the Node's read cursor only until that message is available.
      if (preview.isEmpty && (history['missing'] as List? ?? []).isNotEmpty) {
        final turn = (history['missing'] as List).last;
        var before = entries
            .where((entry) => entry['turn'] == turn)
            .firstOrNull?['sequence'];
        while (mounted && request == _request) {
          if (widget.previewActive?.call() == false) {
            _stamp = null;
            return;
          }
          final result = await host.command('read_turn', {
            'session': session,
            'turn': turn,
            'expected_revision': page['revision'],
            'before': before,
            'limit': 100,
          });
          final chunk = object(result['data']);
          if (!mounted || request != _request) return;
          preview = messagePreview(
            objects(chunk['entries']),
            translate: context.tr,
          );
          final next = chunk['next_before'];
          if (preview.isNotEmpty || next == null) break;
          if (before != null && number(next) >= number(before)) {
            throw const FormatException('message preview did not advance');
          }
          before = next;
        }
      }
      if (mounted && request == _request) {
        _preview = preview;
        if (widget.previewActive?.call() != false) setState(() {});
      }
    } catch (error) {
      debugPrint('Conversation preview unavailable: $error');
      if (mounted && request == _request) {
        _failed = true;
        if (widget.previewActive?.call() != false) setState(() {});
      }
    }
  }

  @override
  Widget build(BuildContext context) {
    final connected = widget.host.connected;
    final session = widget.session;
    final tone = connected ? listTone(session) : null;
    final time = runTime(session);
    return TaskRow(
      icon: ProjectIcon(project: widget.project, size: 26),
      title: title(session, translate: context.tr),
      preview: _preview?.isNotEmpty == true
          ? _preview!
          : _failed
          ? context.tr('conversationPreviewUnavailable')
          : _preview != null
          ? context.tr('conversationNoMessages')
          : connected
          ? ''
          : context.tr('conversationOffline'),
      time: time == null
          ? null
          : timeLabel(
              time,
              widget.now,
              locale: dateLocale(Localizations.localeOf(context)),
              yesterday: context.tr('yesterday'),
            ),
      status: label(session, translate: context.tr),
      tone: tone,
      dim:
          tone == null &&
          object(object(session['activity'])['attention'])['unread'] != true,
      divider: widget.divider,
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
}
