declare const brand: unique symbol;
type Brand<T, B extends string> = T & { readonly [brand]: B };

export type ProjectName = Brand<string, 'ProjectName'>;
export type InstanceName = Brand<string, 'InstanceName'>;
export type BranchName = Brand<string, 'BranchName'>;

export function projectName(raw: string): ProjectName {
  return raw as ProjectName;
}

export function instanceName(raw: string): InstanceName {
  return raw as InstanceName;
}

export function branchName(raw: string): BranchName {
  return raw as BranchName;
}
