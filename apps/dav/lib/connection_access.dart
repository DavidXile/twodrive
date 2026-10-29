import 'package:flutter/material.dart';
import 'client.dart';

class ConnectionAccess extends StatelessWidget {
  const ConnectionAccess({super.key, required this.client, this.profile});
  final StorageClient client;
  final Json? profile;
  @override
  Widget build(BuildContext context) {
    final p = profile ?? client.profile;
    if (p == null) return const SizedBox.shrink();
    final editable = p['writable'] == true;
    final locked = p['write_locked'] == true;
    return OutlinedButton.icon(
      key: ValueKey(
        profile == null ? 'connection-access' : 'access-${p['id']}',
      ),
      icon: Icon(
        editable && !locked ? Icons.edit_outlined : Icons.lock_outline,
        size: 16,
      ),
      label: Text(editable ? (locked ? '已锁定' : '可编辑') : '只读'),
      onPressed: client.busy || !client.available
          ? null
          : () => showDialog<void>(
              context: context,
              builder: (_) => _AccessDialog(client: client, profile: p),
            ),
    );
  }
}

class _AccessDialog extends StatefulWidget {
  const _AccessDialog({required this.client, required this.profile});
  final StorageClient client;
  final Json profile;
  @override
  State<_AccessDialog> createState() => _AccessDialogState();
}

class _AccessDialogState extends State<_AccessDialog> {
  final password = TextEditingController(),
      confirmation = TextEditingController();
  late String mode = widget.profile['writable'] == true
      ? 'editable'
      : 'readonly';
  bool working = false;
  String? error;
  @override
  void dispose() {
    password.dispose();
    confirmation.dispose();
    super.dispose();
  }

  Future<void> apply(String selected) async {
    if (selected == 'editable' &&
        widget.client.status['write_lock']?['configured'] != true &&
        (password.text.length < 8 || password.text != confirmation.text)) {
      setState(() => error = '请设置至少 8 个字符的本机写入密码，并确认两次输入一致');
      return;
    }
    setState(() {
      working = true;
      error = null;
    });
    await widget.client.setAccess(widget.profile, selected, password.text);
    if (!mounted) return;
    if (widget.client.error == null) {
      Navigator.pop(context);
      return;
    }
    setState(() {
      working = false;
      error = widget.client.error;
    });
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.client;
    final provider = c.providers
        .where((d) => d['id'] == widget.profile['kind'])
        .firstOrNull;
    final capabilities = provider?['capabilities'] as Map? ?? {};
    final supported = capabilities['upload'] == true;
    final setup = c.status['write_lock']?['configured'] != true;
    return AlertDialog(
      title: const Text('连接权限'),
      scrollable: true,
      content: SizedBox(
        width: 440,
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text(
              widget.profile['name'] as String,
              style: Theme.of(context).textTheme.titleMedium,
            ),
            const SizedBox(height: 16),
            if (!supported)
              const Text('当前网盘适配器仅支持浏览和下载，尚未提供写入操作。')
            else ...[
              DropdownButtonFormField<String>(
                initialValue: mode,
                decoration: const InputDecoration(labelText: '客户端和挂载权限'),
                items: const [
                  DropdownMenuItem(value: 'readonly', child: Text('只读')),
                  DropdownMenuItem(value: 'editable', child: Text('可编辑')),
                ],
                onChanged: working
                    ? null
                    : (value) => setState(() => mode = value!),
              ),
              const SizedBox(height: 16),
              const Text(
                '解锁仅对这个网盘生效。客户端和文件管理器挂载共用权限；重启后重新锁定，仍可浏览和下载。切换时会重新挂载，请先关闭正在编辑的文件。',
              ),
              if (mode == 'editable') ...[
                const SizedBox(height: 16),
                TextField(
                  controller: password,
                  obscureText: true,
                  enableSuggestions: false,
                  autocorrect: false,
                  enabled: !working,
                  decoration: InputDecoration(
                    labelText: setup ? '设置本机写入密码' : '本机写入密码',
                    helperText: '独立的本机密码，无需填写网盘密码',
                  ),
                ),
                if (setup) ...[
                  const SizedBox(height: 16),
                  TextField(
                    controller: confirmation,
                    obscureText: true,
                    enableSuggestions: false,
                    autocorrect: false,
                    enabled: !working,
                    decoration: const InputDecoration(labelText: '确认写入密码'),
                  ),
                ],
                if (provider?['write_policy'] is String) ...[
                  const SizedBox(height: 16),
                  Text(provider!['write_policy'] as String),
                ],
              ],
            ],
            const SizedBox(height: 16),
            Wrap(
              spacing: 8,
              children: [
                Chip(label: Text('浏览文件')),
                const Chip(label: Text('下载文件')),
                if (widget.profile['id'] == c.selected && c.canUpload)
                  const Chip(label: Text('上传文件')),
                if (widget.profile['id'] == c.selected && c.canMkdir)
                  const Chip(label: Text('新建文件夹')),
                if (widget.profile['id'] == c.selected && c.canRename)
                  const Chip(label: Text('重命名')),
                if (widget.profile['id'] == c.selected && c.canDelete)
                  const Chip(label: Text('删除')),
              ],
            ),
            if (error != null)
              Padding(
                padding: const EdgeInsets.only(top: 12),
                child: Text(
                  error!,
                  style: TextStyle(color: Theme.of(context).colorScheme.error),
                ),
              ),
            if (working)
              const Padding(
                padding: EdgeInsets.only(top: 12),
                child: LinearProgressIndicator(),
              ),
          ],
        ),
      ),
      actions: [
        if (supported &&
            widget.profile['writable'] == true &&
            widget.profile['write_locked'] != true)
          TextButton(
            onPressed: working ? null : () => apply('locked'),
            child: const Text('立即上锁'),
          ),
        TextButton(
          onPressed: working ? null : () => Navigator.pop(context),
          child: Text(supported ? '取消' : '知道了'),
        ),
        if (supported)
          FilledButton(
            onPressed: working ? null : () => apply(mode),
            child: Text(mode == 'editable' ? '解锁并应用' : '应用'),
          ),
      ],
    );
  }
}
