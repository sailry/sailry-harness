# Handoff Summary

{% if user_intent or pending_tasks %}
## Goal
{% for item in user_intent %}
- {{ item }}
{% endfor %}
{% if pending_tasks %}

Open tasks:
{% for item in pending_tasks %}
- {{ item }}
{% endfor %}
{% endif %}

{% endif %}
{% if user_messages %}
## Constraints
{% for item in user_messages %}
- {{ item }}
{% endfor %}

{% endif %}
{% if current_work %}
## State
{{ current_work }}

{% endif %}
{% if problem_solving %}
## Decisions
{% for item in problem_solving %}
- {{ item }}
{% endfor %}

{% endif %}
{% if next_step %}
## Next Steps
{{ next_step }}

{% endif %}
{% if files or errors_and_fixes or technical_concepts %}
## Essential Context
{% for file in files %}
- {% if file.path %}`{{ file.path }}`{% endif %}{% if file.path and file.summary %}: {% endif %}{{ file.summary }}
{% if file.key_code %}
{{ file.key_code | code_fence }}
{% endif %}
{% endfor %}
{% for item in errors_and_fixes %}
- {{ item }}
{% endfor %}
{% for item in technical_concepts %}
- {{ item }}
{% endfor %}
{% endif %}
