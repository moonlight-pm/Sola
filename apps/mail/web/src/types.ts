export interface Folder { name: string; unread: number; total: number; }

export interface MessageSummary {
  uid: number;
  from: string;
  to: string;
  subject: string;
  date: string;
  seen: boolean;
  forwarded_for?: string | null;
}

export interface MessageBody {
  uid: number;
  from: string;
  to: string;
  cc: string;
  subject: string;
  date: string;
  html: string | null;
  text: string;
  in_reply_to: string | null;
  message_id: string | null;
}

export interface MailRuleCondition { field: string; match: string; value: string; }

export interface MailRule {
  name: string;
  action: string;
  dest?: string | null;
  conditions: MailRuleCondition[];
}

export function smartMailboxNames(rules: MailRule[]): string[] {
  return rules.filter(r => r.action === 'smart_mailbox').map(r => r.name);
}

export function matchesRule(
  rule: MailRule,
  msg: Pick<MessageSummary, 'from' | 'subject' | 'to'>
): boolean {
  if (rule.conditions.length === 0) return false;
  return rule.conditions.every(cond => {
    const fieldValue = cond.field === 'from' ? msg.from
      : cond.field === 'subject' ? msg.subject
      : cond.field === 'to' ? msg.to
      : '';
    if (cond.field !== 'from' && cond.field !== 'subject' && cond.field !== 'to') return false;
    const value = cond.value.toLowerCase();
    switch (cond.match) {
      case 'domain': {
        const at = fieldValue.lastIndexOf('@');
        if (at < 0) return false;
        const after = fieldValue.slice(at + 1);
        const domain = after.replace(/>.*/, '').toLowerCase();
        return domain === value;
      }
      case 'address': {
        const lt = fieldValue.indexOf('<');
        if (lt >= 0) {
          const gt = fieldValue.indexOf('>', lt);
          if (gt < 0) return false;
          return fieldValue.slice(lt + 1, gt).trim().toLowerCase() === value;
        }
        return fieldValue.trim().toLowerCase() === value;
      }
      case 'contains':
        return fieldValue.toLowerCase().includes(value);
      case 'equals':
        return fieldValue.toLowerCase() === value;
      default:
        return false;
    }
  });
}

export function matchesAnySmartMailbox(
  rules: MailRule[],
  msg: Pick<MessageSummary, 'from' | 'subject' | 'to'>
): boolean {
  return rules.some(r => r.action === 'smart_mailbox' && matchesRule(r, msg));
}
