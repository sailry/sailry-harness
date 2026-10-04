import '../../l10n/strings.dart';
import '../../runtime/json.dart';

String noticeTitle(Map<String, dynamic> notice) =>
    text(notice['title']).isNotEmpty
    ? text(notice['title'])
    : tr('conversationNew');
