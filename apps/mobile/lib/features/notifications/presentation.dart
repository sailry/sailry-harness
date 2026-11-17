import '../../l10n/strings.dart';
import '../../runtime/json.dart';

String noticeTitle(Map<String, dynamic> notice, {Translator translate = tr}) =>
    object(notice['details'])['type'] == 'personal_kv_conflict'
    ? translate('personalKvConflict')
    : text(notice['title']).isNotEmpty
    ? text(notice['title'])
    : translate('conversationNew');
