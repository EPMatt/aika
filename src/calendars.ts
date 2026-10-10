import type { Calendar } from './generated/Calendar';
export const calendarPalette = [
  ['Sage', '#718675'],
  ['Blue', '#5B7FA3'],
  ['Violet', '#8A6FA8'],
  ['Rose', '#B56F86'],
  ['Amber', '#B38B43'],
  ['Teal', '#4F918A'],
  ['Orange', '#B77B50'],
  ['Slate', '#768390'],
] as const;
export const personalCalendar: Calendar = {
  id: 'local-default',
  providerId: 'local',
  name: 'Personal',
  description: '',
  color: '#718675',
  writable: true,
};
