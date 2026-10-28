import 'package:flutter/material.dart';
import 'client.dart';

class ConnectionAccess extends StatelessWidget {
  const ConnectionAccess({super.key, required this.client});
  final StorageClient client;

  String get explanation {
    final c = client;
    if (c.writable) {
      return '当前连接允许以下操作，实际执行仍需网盘授权。';
    }
    final provider = c.providers
        .where((p) => p['id'] == c.profile?['kind'])
        .firstOrNull;
    final declared = provider?['capabilities'];
    const writeKeys = ['upload', 'mkdir', 'rename', 'delete'];
    if (declared is Map && writeKeys.every((key) => declared[key] == false)) {
      return '当前网盘适配器仅支持浏览和下载，尚未提供写入操作。';
    }
    if (c.profile?['writable'] == false) {
      return '此连接未开启“允许修改远端文件”。如需启用，请先断开连接，再编辑此连接。'
          '可用操作仍取决于网盘支持情况和远端授权。';
    }
    return '当前连接未获得写入能力。可用操作由网盘支持情况和远端授权共同决定。';
  }

  @override
  Widget build(BuildContext context) {
    final c = client;
    return Tooltip(
      message: '查看连接权限',
      child: OutlinedButton.icon(
        key: const ValueKey('connection-access'),
        icon: Icon(
          c.writable ? Icons.edit_outlined : Icons.lock_outline,
          size: 16,
        ),
        label: Row(
          mainAxisSize: MainAxisSize.min,
          children: [
            Text(c.writable ? '读写' : '只读'),
            const SizedBox(width: 6),
            const Icon(Icons.info_outline, size: 14),
          ],
        ),
        onPressed: () => showDialog<void>(
          context: context,
          builder: (context) => AlertDialog(
            title: const Text('连接权限'),
            scrollable: true,
            content: SizedBox(
              width: 420,
              child: Column(
                mainAxisSize: MainAxisSize.min,
                crossAxisAlignment: CrossAxisAlignment.start,
                children: [
                  Text(
                    c.providerLabel,
                    style: Theme.of(context).textTheme.titleMedium,
                  ),
                  const SizedBox(height: 12),
                  Text(explanation),
                  const SizedBox(height: 20),
                  const Text('当前可用操作'),
                  const SizedBox(height: 8),
                  Wrap(
                    spacing: 8,
                    runSpacing: 4,
                    children: [
                      for (final label in [
                        '浏览文件',
                        '下载文件',
                        if (c.canUpload) '上传文件',
                        if (c.canMkdir) '新建文件夹',
                        if (c.canRename) '重命名',
                        if (c.canDelete) '删除',
                      ])
                        Chip(label: Text(label)),
                    ],
                  ),
                ],
              ),
            ),
            actions: [
              TextButton(
                onPressed: () => Navigator.pop(context),
                child: const Text('知道了'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}
