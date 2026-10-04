import 'dart:convert';
import 'package:flutter/material.dart';
import '../../../l10n/strings.dart';
import '../../../runtime/json.dart';
import '../../../runtime/session.dart';
import '../../../ui/kit.dart';
import 'presentation.dart';

class ConversationSearch extends StatefulWidget {
  const ConversationSearch({
    super.key,
    required this.host,
    required this.session,
  });
  final HostConnection host;
  final String session;
  @override
  State<ConversationSearch> createState() => _ConversationSearchState();
}

class _ConversationSearchState extends State<ConversationSearch> {
  final _query = TextEditingController();
  List<Map<String, dynamic>> _matches = [];
  Map<String, dynamic>? _page;
  String? _error;
  bool _busy = false;
  String _searched = '';
  @override
  void dispose() {
    _query.dispose();
    super.dispose();
  }

  Future<void> _search({bool more = false}) async {
    if (_busy) return;
    final query = more ? _searched : _query.text.trim();
    if (query.isEmpty) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final response = object(
        jsonDecode(
          await widget.host.connection.searchConversation(
            session: widget.session,
            query: jsonEncode({
              'text': query,
              'case_sensitive': false,
              'before': more ? (_page?['next_before']) : null,
              'limit': 30,
            }),
          ),
        ),
      );
      if (!mounted) return;
      if (response['Err'] != null) {
        throw CommandFailure(text(object(response['Err'])['code']));
      }
      final page = object(response['Ok']);
      if (page['session'] != widget.session ||
          more && page['revision'] != _page?['revision']) {
        throw tr('messageSearchStale');
      }
      setState(() {
        _matches = [if (more) ..._matches, ...objects(page['matches'])];
        _page = page;
        _searched = query;
      });
    } catch (failure) {
      if (mounted) setState(() => _error = failureLabel(failure));
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) => PageFrame(
    title: tr('messageSearch'),
    loading: _busy,
    actions: [
      IconButton(
        tooltip: tr('messageSearch'),
        onPressed: _busy ? null : _search,
        icon: const AppIcon('search'),
      ),
    ],
    child: Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        TextField(
          controller: _query,
          enabled: !_busy,
          textInputAction: TextInputAction.search,
          onSubmitted: (_) => _search(),
          decoration: InputDecoration(hintText: tr('messageSearchHint')),
        ),
        if (_error != null) FailureState(message: _error!, onRetry: _search),
        for (final match in _matches)
          ListTile(
            title: SearchSnippet(match: match),
            subtitle: Text(text(match['author'])),
            onTap: () => Navigator.pop(context, {
              ...match,
              'revision': _page!['revision'],
            }),
          ),
        if (_page != null && _matches.isEmpty && _error == null)
          Text(tr('messageNoResults')),
        if (_page?['next_before'] != null)
          TextButton(
            onPressed: _busy ? null : () => _search(more: true),
            child: Text(tr('resourceMore')),
          ),
      ],
    ),
  );
}

/// Search offsets are UTF-8 bytes supplied by the shared Client.
class SearchSnippet extends StatelessWidget {
  const SearchSnippet({super.key, required this.match});
  final Map<String, dynamic> match;
  @override
  Widget build(BuildContext context) {
    final value = text(match['snippet']);
    final bytes = utf8.encode(value);
    final range = object(match['highlight']);
    final start = number(range['start']).toInt().clamp(0, bytes.length);
    final end = number(range['end']).toInt().clamp(start, bytes.length);
    return Text.rich(
      TextSpan(
        children: [
          TextSpan(text: utf8.decode(bytes.sublist(0, start))),
          TextSpan(
            text: utf8.decode(bytes.sublist(start, end)),
            style: TextStyle(
              backgroundColor: Theme.of(context).colorScheme.primaryContainer,
              fontWeight: FontWeight.w600,
            ),
          ),
          TextSpan(text: utf8.decode(bytes.sublist(end))),
        ],
      ),
    );
  }
}
