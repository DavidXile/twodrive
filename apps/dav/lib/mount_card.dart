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
          Text('挂载为只读文件夹，可浏览、打开和复制文件。'),
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
  const MountCard({super.key, required this.client, required this.onOpen});
  final StorageClient client;
  final VoidCallback onOpen;
  @override
  Widget build(BuildContext context) {
    final c = client;
    return Card(
      color: Colors.white,
      child: Padding(
        padding: const EdgeInsets.all(20),
        child: Column(
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              c.readOnlyMount ? '文件管理器挂载 · 只读' : '文件管理器挂载',
              style: Theme.of(context).textTheme.titleLarge,
            ),
            const SizedBox(height: 10),
            SelectableText(c.profile?['mount_path'] as String? ?? '请先选择连接'),
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
                          (!c.connected && !c.mounted) ||
                          (!c.canMount && !c.mounted)
                      ? null
                      : c.mountToggle,
                  icon: Icon(c.mounted ? Icons.eject : Icons.folder_open),
                  label: Text(c.mounted ? '卸载' : '挂载'),
                ),
                OutlinedButton(
                  onPressed: c.mounted ? onOpen : null,
                  child: const Text('在文件管理器中打开'),
                ),
              ],
            ),
            const SizedBox(height: 12),
            Text(
              c.readOnlyMount
                  ? '可浏览、打开和复制文件，不能在挂载内修改或删除。首次打开文件需下载到临时缓存，较大文件可能需要等待。切换网盘保留挂载；卸载或退出前，请先完成资源管理器中的文件复制。'
                  : '挂载按需下载。写入先保存到本地缓存，待上传完成后才代表服务器已收到。',
            ),
          ],
        ),
      ),
    );
  }
}
