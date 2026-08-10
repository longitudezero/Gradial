return {
    begin = function(context, parameters, initial)
        return {
            action_id = context.action_id,
            last_value = initial.value,
            update_count = 0,
        }
    end,

    update = function(session, sample)
        session.last_value = sample.value
        session.update_count = session.update_count + 1
    end,

    commit = function(session, final_sample)
        session.last_value = final_sample.value
    end,

    cancel = function(session, reason)
        session.cancel_reason = reason
    end,
}
