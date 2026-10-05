import 'dart:async';
import 'dart:convert';

import 'package:flutter/material.dart';
import 'package:sailry_bridge/api/commands.dart';

import '../../l10n/strings.dart';
import '../../runtime/session.dart';
import '../../runtime/ports.dart';
import '../../runtime/json.dart';
import '../../runtime/notices.dart';
import '../../ui/kit.dart';
import 'port_preview.dart';

class PortsPage extends StatefulWidget {
  const PortsPage({super.key, required this.host, this.session});
  final HostConnection host;
  final String? session;
  @override
  State<PortsPage> createState() => _PortsPageState();
}

class _PortsPageState extends State<PortsPage> {
  final _port = TextEditingController();
  late final _mappings = widget.host.ports;
  CommandUpdates? _updates;
  Map<String, dynamic> _services = {};
  String? _error;
  bool _busy = false;
  bool _previewOpen = false;
  bool _watching = false;

  @override
  void initState() {
    super.initState();
    if (widget.session != null) unawaited(_watchServices());
  }

  Future<void> _watchServices() async {
    if (_watching) return;
    _watching = true;
    CommandUpdates? updates;
    try {
      updates = await widget.host.connection.watchCommands(
        session: widget.session!,
      );
      if (!mounted) return;
      _updates = updates;
      while (mounted) {
        final view = object(jsonDecode(await updates.next()));
        if (!mounted) return;
        setState(() => _services = view);
      }
    } catch (_) {
      if (mounted) setState(() => _services = {'connected': false});
    } finally {
      _watching = false;
      _updates = null;
      if (updates != null) await updates.close().whenComplete(updates.dispose);
    }
  }

  @override
  void dispose() {
    _port.dispose();
    final updates = _updates;
    if (updates != null) unawaited(updates.close());
    super.dispose();
  }

  Future<void> _open({Map<String, dynamic>? source}) async {
    if (_busy || _mappings.busy) return;
    final port = source == null
        ? int.tryParse(_port.text)
        : object(source['service'])['port'] as int?;
    if (port == null || port < 1 || port > 65535) {
      setState(() => _error = context.tr('resourceInvalidPort'));
      return;
    }
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final mapping = await _mappings.open(port, source: source);
      if (mounted && source != null) {
        final uri = Uri.parse(
          text(object(source['service'])['url']),
        ).replace(host: '127.0.0.1', port: mapping.local);
        await _browse(mapping, uri: uri);
      }
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  Future<void> _close(PortMapping entry) async {
    try {
      await _mappings.remove(entry.remote);
    } catch (error) {
      if (mounted) {
        setState(() => _error = failureText(error, translate: context.tr));
      }
    }
  }

  Future<void> _browse(PortMapping entry, {Uri? uri}) async {
    if (!entry.listening.value || _previewOpen) return;
    _previewOpen = true;
    try {
      await pushPage(
        context,
        PortPreviewPage(uri: uri ?? entry.uri, listening: entry.listening),
      );
    } finally {
      _previewOpen = false;
    }
  }

  List<Widget> _serviceRows() {
    final seen = <int>{};
    return [
      for (final command in objects(_services['items']))
        for (final service in objects(command['services']))
          if (service['port'] is int && seen.add(service['port'] as int))
            ListTile(
              key: ValueKey('service-${service['port']}'),
              title: Text(
                _mappings.entries[service['port']]?.listening.value == true
                    ? '${service['port']} → ${_mappings.entries[service['port']]!.local}'
                    : '${service['port']}',
              ),
              subtitle: Text(text(service['url'])),
              trailing: IconButton(
                tooltip: context.tr('resourceServiceOpen'),
                icon: const AppIcon('link'),
                onPressed:
                    _busy || _mappings.busy || _services['connected'] != true
                    ? null
                    : () => _open(
                        source: {
                          'session': widget.session,
                          'command': command['id'],
                          'service': service,
                        },
                      ),
              ),
            ),
    ];
  }

  @override
  Widget build(BuildContext context) => ListenableBuilder(
    listenable: _mappings,
    builder: (context, _) {
      final rows = _serviceRows();
      return PageFrame(
        title: context.tr('ports'),
        loading: _busy || _mappings.busy,
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.stretch,
          children: [
            if (widget.session != null) ...[
              Surface(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      context.tr('resourceSessionServices'),
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    if (_services.isEmpty)
                      const LinearProgressIndicator()
                    else if (_services['connected'] != true)
                      TextButton(
                        onPressed: _watchServices,
                        child: Text(context.tr('resourceServicesUnavailable')),
                      )
                    else if (rows.isEmpty)
                      Text(context.tr('resourceNoServices')),
                    ...rows,
                  ],
                ),
              ),
              const SizedBox(height: 16),
            ],
            for (final entry in _mappings.entries.values) ...[
              Surface(
                child: Column(
                  crossAxisAlignment: CrossAxisAlignment.stretch,
                  children: [
                    Text(
                      '${entry.remote} → ${entry.local}',
                      style: Theme.of(context).textTheme.titleMedium,
                    ),
                    SelectableText(entry.uri.toString()),
                    if (!entry.listening.value)
                      Text(context.tr('resourceForwardStopped')),
                    if (entry.error != null)
                      Text(failureText(entry.error!, translate: context.tr)),
                    FilledButton(
                      onPressed: entry.listening.value
                          ? () => _browse(entry)
                          : null,
                      child: Text(context.tr('resourceOpenBrowser')),
                    ),
                    OutlinedButton(
                      onPressed: () => _close(entry),
                      child: Text(context.tr('closePort')),
                    ),
                  ],
                ),
              ),
              const SizedBox(height: 16),
            ],
            Surface(
              child: Column(
                crossAxisAlignment: CrossAxisAlignment.stretch,
                children: [
                  TextField(
                    controller: _port,
                    enabled: !_busy && !_mappings.busy,
                    keyboardType: TextInputType.number,
                    decoration: InputDecoration(
                      labelText: context.tr('resourceRemotePort'),
                    ),
                  ),
                  if (_error != null)
                    Padding(
                      padding: const EdgeInsets.only(top: 12),
                      child: Text(
                        _error!,
                        style: TextStyle(
                          color: Theme.of(context).colorScheme.error,
                        ),
                      ),
                    ),
                ],
              ),
            ),
            const SizedBox(height: 16),
            FilledButton(
              onPressed: _busy || _mappings.busy ? null : _open,
              child: Text(context.tr('resourceOpenPort')),
            ),
          ],
        ),
      );
    },
  );
}
