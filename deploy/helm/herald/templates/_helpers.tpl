{{- define "herald.fullname" -}}
{{- if contains .Chart.Name .Release.Name -}}
{{- .Release.Name | trunc 63 | trimSuffix "-" -}}
{{- else -}}
{{- printf "%s-%s" .Release.Name .Chart.Name | trunc 63 | trimSuffix "-" -}}
{{- end -}}
{{- end -}}

{{- define "herald.selectorLabels" -}}
app.kubernetes.io/name: {{ .Chart.Name }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end -}}

{{- define "herald.labels" -}}
{{ include "herald.selectorLabels" . }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
helm.sh/chart: {{ printf "%s-%s" .Chart.Name .Chart.Version }}
{{- end -}}

{{- define "herald.image" -}}
{{- $tag := default .Chart.AppVersion .Values.image.tag -}}
{{- if .Values.image.digest -}}
{{- printf "%s:%s@%s" .Values.image.repository $tag .Values.image.digest -}}
{{- else -}}
{{- printf "%s:%s" .Values.image.repository $tag -}}
{{- end -}}
{{- end -}}

{{- define "herald.table" -}}
{{- $name := index . 0 -}}
{{- $values := index . 1 -}}
{{- $set := dict -}}
{{- range $key, $value := $values -}}
{{- if and (not (kindIs "map" $value)) (not (kindIs "invalid" $value)) -}}
{{- $_ := set $set $key $value -}}
{{- end -}}
{{- end -}}
{{- if $set }}

[{{ $name }}]
{{- range $key, $value := $set }}
{{ $key }} = {{ toJson $value }}
{{- end }}
{{- end -}}
{{- end -}}

{{- define "herald.toml" -}}
{{ .Values.config | trimSuffix "\n" }}
{{- include "herald.table" (list "server" .Values.server) }}
{{- include "herald.table" (list "server.keys" .Values.server.keys) }}
{{- include "herald.table" (list "schedule" .Values.schedule) }}
{{ end -}}

{{- define "herald.secretName" -}}
{{- default (printf "%s-secrets" (include "herald.fullname" .)) .Values.secrets.existingSecret -}}
{{- end -}}
