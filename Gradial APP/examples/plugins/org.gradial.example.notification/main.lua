return {
    begin = function(context, parameters, initial)
        return {
            action_id = context.action_id,
            last_value = initial.value,
        }
    end,

    update = function(session, sample)
        session.last_value = sample.value
    end,

    commit = function(session, final_sample)
        session.last_value = final_sample.value
        local accepted, reason = gradial.notification("Continuous gesture committed.")
        session.notification_status = accepted and "accepted" or reason
    end,

    cancel = function(session, reason)
        session.cancel_reason = reason
    end,
}
