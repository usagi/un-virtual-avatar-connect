import type { ReadinessSummary, ReadinessStatus } from './types';

// A mode's required checks decide readiness to run, but the home panel also
// reports attention on optional features the user has actually configured.
export function visibleReadinessStatus(summary: ReadinessSummary, requiredOnly = false): ReadinessStatus {
  const checks = summary.checks.filter(check => !requiredOnly || check.required);
  if (checks.some(check => check.status === 'blocked')) return 'blocked';
  if (checks.some(check => check.status === 'attention')) return 'attention';
  return summary.status;
}
