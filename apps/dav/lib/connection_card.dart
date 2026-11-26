import 'language.dart';
import 'package:flutter/material.dart';
import 'client.dart';
import 'storage_usage.dart';
import 'workspace_style.dart';

class ConnectionCard extends StatelessWidget {
  const ConnectionCard({
    super.key,
    required this.profile,
    required this.online,
    required this.selected,
    required this.connectLabel,
    this.usage,
    this.issue,
    this.onSelect,
    this.onReauthenticate,
    this.onDisconnect,
    this.onEdit,
    this.onDelete,
    this.onRefresh,
    this.mountSupported = false,
    this.mounted = false,
    this.onMount,
    this.onOpenMount,
    this.mountPath,
    this.mountFailed = false,
  });
  final Json profile;
  final bool online, selected;
  final bool mountSupported, mounted;
  final String? mountPath;
  final bool mountFailed;
  final String connectLabel;
  final Json? usage;
  final String? issue;
  final VoidCallback? onSelect, onDisconnect, onEdit, onDelete, onRefresh;
  final VoidCallback? onReauthenticate;
  final VoidCallback? onMount, onOpenMount;
  @override
  Widget build(BuildContext context) {
    final provider =
        profile['provider_label'] as String? ?? profile['kind'] as String;
    final account = profile['account_name'] as String?;
    final description = [
      tr(provider),
      if (online) tr('已连接'),
      if (account != null &&
          account.isNotEmpty &&
          account != profile['name'] &&
          account != provider)
        '${tr(profile['account_label'] as String? ?? '账号')}：$account',
      if (profile['summary'] != null &&
          profile['summary'] != provider &&
          profile['summary'] != profile['name'])
        profile['summary'] as String,
    ].join(' · ');
    Widget button(String label, IconData icon, VoidCallback? callback) =>
        OutlinedButton(
          onPressed: callback,
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(icon, size: 18),
              const SizedBox(width: 6),
              Flexible(
                child: FittedBox(
                  fit: BoxFit.scaleDown,
                  child: Text(tr(label), maxLines: 1, softWrap: false),
                ),
              ),
            ],
          ),
        );
    Widget actionRow(Widget left, Widget? right) => Row(
      children: [
        Expanded(child: left),
        if (right != null) ...[
          const SizedBox(width: 8),
          Expanded(child: right),
        ],
      ],
    );
    final reauthenticate = profile['can_reauthenticate'] == true
        ? button('重新登录', Icons.refresh, onReauthenticate)
        : null;
    final actions = Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        actionRow(
          button(
            connectLabel,
            online ? Icons.folder_open_outlined : Icons.login,
            onSelect,
          ),
          mountSupported
              ? button(
                  mounted ? '卸载' : '挂载',
                  mounted ? Icons.eject_outlined : Icons.add_to_drive_outlined,
                  onMount,
                )
              : reauthenticate,
        ),
        if (mountSupported) ...[
          const SizedBox(height: 6),
          actionRow(
            button('打开挂载', Icons.folder_open, onOpenMount),
            reauthenticate ?? const SizedBox.shrink(),
          ),
        ],
      ],
    );
    final managementActions = {
      '断开此连接': (Icons.link_off, online ? onDisconnect : null),
      '编辑': (Icons.edit_outlined, onEdit),
      '删除配置': (Icons.delete_outline, onDelete),
    };
    final title = Row(
      children: [
        Expanded(
          child: Tooltip(
            message: profile['name'] as String,
            child: Text(
              profile['name'] as String,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: Theme.of(context).textTheme.titleMedium,
            ),
          ),
        ),
        if (selected) ...[
          const SizedBox(width: 8),
          Chip(
            visualDensity: VisualDensity.compact,
            avatar: Icon(Icons.check_circle, size: 18, color: Colors.blue),
            label: Text(tr('当前使用')),
          ),
        ],
        const SizedBox(width: 8),
        SizedBox(
          width: 40,
          child: PopupMenuButton<String>(
            tooltip: tr('更多操作'),
            icon: const Icon(Icons.more_horiz),
            padding: EdgeInsets.zero,
            enabled: managementActions.values.any(
              (action) => action.$2 != null,
            ),
            onSelected: (label) => managementActions[label]?.$2?.call(),
            itemBuilder: (context) => [
              for (final MapEntry(key: label, value: (icon, callback))
                  in managementActions.entries)
                PopupMenuItem<String>(
                  value: label,
                  enabled: callback != null,
                  child: Row(
                    children: [
                      Icon(icon, size: 18),
                      const SizedBox(width: 10),
                      Flexible(child: Text(tr(label))),
                    ],
                  ),
                ),
            ],
          ),
        ),
      ],
    );
    final details = Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      mainAxisSize: MainAxisSize.min,
      children: [
        Tooltip(
          message: description,
          child: Text(
            description,
            style: const TextStyle(color: Colors.blueGrey),
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
          ),
        ),
        if (issue != null)
          Text(issue!, style: const TextStyle(color: Colors.deepOrange)),
        const SizedBox(height: 8),
        StorageUsageView(value: usage, connected: online, onRefresh: onRefresh),
        if (mountSupported) ...[
          const SizedBox(height: 12),
          Row(
            children: [
              Icon(
                mountFailed
                    ? Icons.error_outline
                    : mounted
                    ? Icons.check_circle_outline
                    : Icons.folder_outlined,
                size: 15,
                color: mountFailed
                    ? Colors.orange
                    : mounted
                    ? successColor
                    : mutedColor,
              ),
              const SizedBox(width: 7),
              Text(
                mountFailed
                    ? tr('挂载需要处理')
                    : mounted
                    ? tr('已挂载')
                    : tr('未挂载'),
                style: const TextStyle(fontSize: 12, color: mutedColor),
              ),
            ],
          ),
          if (mountPath != null && mountPath!.isNotEmpty)
            Padding(
              padding: const EdgeInsets.only(top: 6),
              child: SelectableText(
                mountPath!,
                style: const TextStyle(fontSize: 11, color: mutedColor),
              ),
            ),
        ],
      ],
    );
    final scale = (MediaQuery.textScalerOf(context).scale(14) / 14).clamp(
      1.0,
      3.0,
    );
    final height = (mountSupported ? 312.0 : 225.0) + (scale - 1) * 128;
    final color = providerColor(profile['kind'] as String?);
    return SizedBox(
      height: height,
      child: Card(
        margin: EdgeInsets.zero,
        color: panelColor,
        child: InkWell(
          onTap: onSelect,
          borderRadius: BorderRadius.circular(26),
          child: Padding(
            padding: const EdgeInsets.all(16),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.stretch,
              children: [
                SizedBox(
                  height: 38 + (scale - 1) * 16,
                  child: Row(
                    children: [
                      Container(
                        width: 38,
                        height: 38,
                        decoration: BoxDecoration(
                          color: color.withValues(alpha: .09),
                          borderRadius: BorderRadius.circular(12),
                        ),
                        child: Icon(
                          profile['icon'] == 'devices'
                              ? Icons.devices
                              : providerIcon(profile['kind'] as String?),
                          size: 25,
                          color: color,
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(child: title),
                    ],
                  ),
                ),
                const SizedBox(height: 8),
                Expanded(
                  child: SingleChildScrollView(primary: false, child: details),
                ),
                const SizedBox(height: 12),
                actions,
              ],
            ),
          ),
        ),
      ),
    );
  }
}
