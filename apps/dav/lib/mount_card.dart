import 'package:flutter/material.dart';
import 'client.dart';

Future<void> showMountHelp(BuildContext context) => showDialog<void>(
  context: context,
  builder: (context) => AlertDialog(
    title: const Text('安装挂载组件'),
    scrollable: true,
    content: const SizedBox(
      width: 420,
      child: Column(
        mainAxisSize: MainAxisSize.min,
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Text('Windows 挂载需要 WinFsp。请从官方页面下载安装程序，完成后回到此处重试；已有网盘登录会保留。'),
          SizedBox(height: 12),
          SelectableText('https://winfsp.dev/rel/'),
          SizedBox(height: 12),
          Text('默认只读；在网盘的连接权限中解锁后，客户端和挂载都允许编辑。'),
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
);

class MountCard extends StatelessWidget {
  const MountCard({
    super.key,
    required this.client,
    required this.onOpen,
    this.profileId,
  });
  final StorageClient client;
  final VoidCallback onOpen;
  final String? profileId;
  @override
  Widget build(BuildContext context) {
    final c = client;
    final id = profileId ?? c.selected;
    final p = c.profiles.where((p) => p['id'] == id).firstOrNull;
    final mount = id == null ? null : c.mountForProfile(id);
    final isMounted = mount != null;
    final readOnly = mount?['writable'] != true;
    return Card(
      color: Colors.white,
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            if (profileId != null)
              Text(
                p?['name'] as String? ?? '网盘',
                style: Theme.of(context).textTheme.titleMedium,
              ),
            Text(
              readOnly ? '文件管理器挂载 · 只读' : '文件管理器挂载',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 10),
            SelectableText(
              mount?['path'] as String? ??
                  p?['mount_path'] as String? ??
                  '请先选择连接',
            ),
            const SizedBox(height: 12),
            if (c.mountDriverMissing) ...[
              const Text('尚未安装 WinFsp，安装后即可挂载。'),
              TextButton(
                onPressed: () => showMountHelp(context),
                child: const Text('查看安装说明'),
              ),
            ],
            Wrap(
              spacing: 12,
              runSpacing: 8,
              children: [
                FilledButton.tonalIcon(
                  onPressed:
                      c.busy ||
                          (id == null || (!c.canMountProfile(id) && !isMounted))
                      ? null
                      : () => c.mountToggle(id),
                  icon: Icon(isMounted ? Icons.eject : Icons.folder_open),
                  label: Text(isMounted ? '卸载' : '挂载'),
                ),
                OutlinedButton(
                  onPressed: isMounted && mount['failed'] != true
                      ? onOpen
                      : null,
                  child: const Text('打开挂载'),
                ),
              ],
            ),
            if (id != null) ...[
              const SizedBox(height: 12),
              OutlinedButton(
                onPressed: () => c.openWriteRecovery(id),
                child: const Text('打开写入恢复目录'),
              ),
            ],
            const SizedBox(height: 8),
            const Text(
              '恢复目录中的 before.bin 是原文件，edited.bin 或 .content 是保留的编辑内容，record.json 记录原路径。请先复制到其他位置，再恢复文件。',
            ),
            const SizedBox(height: 12),
            Text(
              readOnly
                  ? '可浏览、打开和复制文件，不能在挂载内修改或删除。首次打开文件需下载到临时缓存，较大文件可能需要等待。切换网盘保留挂载；卸载或退出前，请先完成资源管理器中的文件复制。'
                  : '挂载按需下载。写入先保存到本地缓存，待上传完成后才代表服务器已收到。',
            ),
          ],
        ),
      ),
    );
  }
}
