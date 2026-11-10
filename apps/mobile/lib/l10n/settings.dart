const settingsStrings = <String, String>{
  'required': '请填写内容',
  'sshAddress': '地址',
  'sshUser': '用户名',
  'connectionName': '连接名称',
  'newConnection': '添加连接',
  'connectionsCount': '{count} 个连接',
};

String lookup(String key) => settingsStrings[key] ?? key;
