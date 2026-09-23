package notes

import (
	"os"
	"path/filepath"
	"strings"
)

type Note struct {
	ID        string
	Name      string
	FilePath  string
	Namespace string
}

func DefaultNotesDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".local", "state", "hornet", "notes")
}

func LegacyDbeeNotesDir() string {
	home, _ := os.UserHomeDir()
	return filepath.Join(home, ".local", "state", "nvim", "dbee", "notes")
}

// ListGlobalNotes returns all .sql files in ~/.local/state/hornet/notes/global/
// falling back to legacy ~/.local/state/nvim/dbee/notes/global/
func ListGlobalNotes() ([]Note, error) {
	dir := filepath.Join(DefaultNotesDir(), "global")
	_ = os.MkdirAll(dir, 0755)

	seen := make(map[string]bool)
	var results []Note

	// 1. Read from Hornet notes dir
	if entries, err := os.ReadDir(dir); err == nil {
		for _, e := range entries {
			if !e.IsDir() && strings.HasSuffix(e.Name(), ".sql") {
				name := strings.TrimSuffix(e.Name(), ".sql")
				seen[name] = true
				results = append(results, Note{
					ID:        e.Name(),
					Name:      name,
					FilePath:  filepath.Join(dir, e.Name()),
					Namespace: "global",
				})
			}
		}
	}

	// 2. Read from legacy nvim-dbee notes dir if present
	legacyDir := filepath.Join(LegacyDbeeNotesDir(), "global")
	if entries, err := os.ReadDir(legacyDir); err == nil {
		for _, e := range entries {
			if !e.IsDir() && strings.HasSuffix(e.Name(), ".sql") {
				name := strings.TrimSuffix(e.Name(), ".sql")
				if !seen[name] {
					seen[name] = true
					results = append(results, Note{
						ID:        e.Name(),
						Name:      name,
						FilePath:  filepath.Join(legacyDir, e.Name()),
						Namespace: "global",
					})
				}
			}
		}
	}

	return results, nil
}

func ReadNote(filePath string) (string, error) {
	data, err := os.ReadFile(filePath)
	if err != nil {
		return "", err
	}
	return string(data), nil
}

func SaveNote(name, content string) (string, error) {
	if !strings.HasSuffix(name, ".sql") {
		name += ".sql"
	}
	dir := filepath.Join(DefaultNotesDir(), "global")
	_ = os.MkdirAll(dir, 0755)

	filePath := filepath.Join(dir, name)
	if err := os.WriteFile(filePath, []byte(content), 0644); err != nil {
		return "", err
	}
	return filePath, nil
}
