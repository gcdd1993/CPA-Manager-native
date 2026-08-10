//go:build windows

package main

import "golang.org/x/sys/windows"

func replaceFile(source string, target string) error {
	sourcePath, errSource := windows.UTF16PtrFromString(source)
	if errSource != nil {
		return errSource
	}
	targetPath, errTarget := windows.UTF16PtrFromString(target)
	if errTarget != nil {
		return errTarget
	}
	return windows.MoveFileEx(sourcePath, targetPath, windows.MOVEFILE_REPLACE_EXISTING|windows.MOVEFILE_WRITE_THROUGH)
}
