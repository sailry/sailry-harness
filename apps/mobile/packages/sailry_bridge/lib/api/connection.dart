import '../src/native.dart';
import 'commands.dart';
import 'connectors.dart';
import 'conversation.dart';
import 'login.dart';
import 'ports.dart';
import 'status.dart';
import 'terminal.dart';
import 'transfers/download.dart';
import 'transfers/upload.dart';
import 'usage.dart';

class Connection extends NativeHandle {
  Connection.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> prepare({required String command}) async =>
      await invoke('prepare', {'command': command}) as String;

  Future<String> execute({required String request}) async =>
      await invoke('execute', {'request': request}) as String;

  Future<String> prepareStatus({
    required String scope,
    required String entry,
  }) async =>
      await invoke('prepare_status', {'scope': scope, 'entry': entry}) as String;

  Future<String> searchConversation({
    required String session,
    required String query,
  }) async =>
      await invoke('search_conversation', {'session': session, 'query': query})
          as String;

  Future<Forwarding> forwardPort({
    required int remotePort,
    required int localPort,
  }) async {
    final value = await invoke('forward_port', {
      'remote_port': remotePort,
      'local_port': localPort,
    });
    return Forwarding.internal(bridge, nativeHandle(value, 'forwarding'));
  }

  Future<Forwarding> forwardService({
    required String source,
    required int localPort,
  }) async {
    final value = await invoke('forward_service', {
      'source': source,
      'local_port': localPort,
    });
    return Forwarding.internal(bridge, nativeHandle(value, 'forwarding'));
  }

  Future<Upload> uploadAttachment({required String upload}) async {
    final value = await invoke('upload_attachment', {'upload': upload});
    return Upload.internal(bridge, nativeHandle(value, 'upload'));
  }

  Future<Download> downloadAttachment({required String download}) async {
    final value = await invoke('download_attachment', {'download': download});
    return Download.internal(bridge, nativeHandle(value, 'download'));
  }

  Future<Download> downloadFile({required String download}) async {
    final value = await invoke('download_file', {'download': download});
    return Download.internal(bridge, nativeHandle(value, 'download'));
  }

  Future<Updates> watch() async {
    final value = await invoke('watch');
    return Updates.internal(bridge, nativeHandle(value, 'updates'));
  }

  Future<CommandUpdates> watchCommands({required String session}) async {
    final value = await invoke('watch_commands', {'session': session});
    return CommandUpdates.internal(bridge, nativeHandle(value, 'command_updates'));
  }

  Future<ConnectionsUpdates> watchConnections() async {
    final value = await invoke('watch_connections');
    return ConnectionsUpdates.internal(
      bridge,
      nativeHandle(value, 'connections_updates'),
    );
  }

  Future<LoginUpdates> watchConnectorLogin({required String attempt}) async {
    final value = await invoke('watch_connector_login', {'attempt': attempt});
    return LoginUpdates.internal(bridge, nativeHandle(value, 'login_updates'));
  }

  Future<ConversationUpdates> watchConversation({required String session}) async {
    final value = await invoke('watch_conversation', {'session': session});
    return ConversationUpdates.internal(
      bridge,
      nativeHandle(value, 'conversation_updates'),
    );
  }

  Future<LoginUpdates> watchLogin({required String attempt}) async {
    final value = await invoke('watch_login', {'attempt': attempt});
    return LoginUpdates.internal(bridge, nativeHandle(value, 'login_updates'));
  }

  Future<StatusUpdates> watchStatus({required String scope}) async {
    final value = await invoke('watch_status', {'scope': scope});
    return StatusUpdates.internal(bridge, nativeHandle(value, 'status_updates'));
  }

  Future<TerminalUpdates> watchTerminal({required String terminal}) async {
    final value = await invoke('watch_terminal', {'terminal': terminal});
    return TerminalUpdates.internal(
      bridge,
      nativeHandle(value, 'terminal_updates'),
    );
  }

  Future<UsageUpdates> watchUsage({required String query}) async {
    final value = await invoke('watch_usage', {'query': query});
    return UsageUpdates.internal(bridge, nativeHandle(value, 'usage_updates'));
  }
}

class Updates extends NativeHandle {
  Updates.internal(super.bridge, super.handle);

  Future<void> close() => closeNative();

  Future<String> next() async => await nextValue() as String;
}
